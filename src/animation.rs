use std::{
    collections::VecDeque,
    fs::FileType,
    path::{Path, PathBuf},
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex,
    },
    thread::JoinHandle,
};

use anyhow::ensure;
use femtovg::{renderer::OpenGl, Canvas, ImageId};

enum LoaderRequest {
    LoadIamge(usize),
    Close,
}

enum LoaderResponse {
    ImageLoaded(image::DynamicImage, usize),
    Error(String),
}

pub struct VgAnimation {
    image_buffer: VecDeque<femtovg::ImageId>,
    current_image: usize,
    loop_count: usize,
    looped_counter: usize,
    frame_time: f64,
    frame_timer: f64,
    compressed: bool,
    canvas: Arc<Mutex<Canvas<OpenGl>>>,
    loader_tx: Sender<LoaderRequest>,
    loader_rx: Receiver<LoaderResponse>,
    loader_thread: JoinHandle<()>,
    image_count: usize,
}

fn loader(rx: Receiver<LoaderRequest>, tx: Sender<LoaderResponse>, paths: Vec<PathBuf>) {
    while let Ok(request) = rx.recv() {
        match request {
            LoaderRequest::LoadIamge(index) => {
                match image::open(&paths[index]) {
                    Ok(img) => tx.send(LoaderResponse::ImageLoaded(img, index)).unwrap(),
                    Err(err) => tx
                        .send(LoaderResponse::Error(format!("{:?}", err)))
                        .unwrap(),
                };
            }
            LoaderRequest::Close => return,
        }
    }
}

impl VgAnimation {
    pub fn new(
        image_root: impl AsRef<Path>,
        frame_time: f64,
        canvas: Arc<Mutex<Canvas<OpenGl>>>,
        loop_count: usize,
        compressed: bool,
    ) -> anyhow::Result<Self> {
        let walker = walkdir::WalkDir::new(image_root.as_ref());

        let image_paths: Vec<_> = walker
            .max_depth(1)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|x| x.ok())
            .filter(|dir| dir.file_type().is_file())
            .map(|dir| dir.into_path())
            .collect();

        let image_count = image_paths.len();

        //Load fiirst image in folder

        ensure!(
            !image_paths.is_empty(),
            "Empty animation folder: {:?}",
            image_root.as_ref()
        );

        let first_img = {
            let mut canvas = canvas.lock().unwrap();
            canvas.load_image_file(&image_paths[0], femtovg::ImageFlags::empty())
        }?;

        let mut image_buffer = VecDeque::new();
        image_buffer.push_back(first_img);

        let (request_tx, request_rx) = channel::<LoaderRequest>();
        let (response_tx, response_rx) = channel::<LoaderResponse>();
        let loader_thread =
            std::thread::spawn(move || loader(request_rx, response_tx, image_paths));

        if compressed {
            request_tx.send(LoaderRequest::LoadIamge(1))?;
        } else {
            image_buffer.resize(image_count, first_img);
            for i in 1..image_count {
                request_tx.send(LoaderRequest::LoadIamge(i))?;
            }
            request_tx.send(LoaderRequest::Close)?;
        }

        Ok(Self {
            image_count,
            image_buffer,
            current_image: 0,
            loop_count,
            looped_counter: 0,
            frame_time,
            frame_timer: frame_time,
            compressed,
            canvas,
            loader_rx: response_rx,
            loader_tx: request_tx,
            loader_thread,
        })
    }

    pub fn tick(&mut self, dt: f64) {
        if self.loop_count > 0 && self.looped_counter >= self.loop_count {
            return;
        }

        self.frame_timer -= dt;

        if !self.compressed {
            match self.loader_rx.try_recv() {
                Ok(LoaderResponse::ImageLoaded(img, idx)) if img.width() > 0 => {
                    let mut canvas = self.canvas.lock().unwrap();
                    let image_id = canvas
                        .create_image(
                            femtovg::ImageSource::try_from(&img).expect("bad image format?"),
                            femtovg::ImageFlags::empty(),
                        )
                        .expect("Failed to create image");

                    self.image_buffer[idx] = image_id
                }
                Ok(LoaderResponse::Error(err)) => {
                    log::warn!("Failed to load animation frame: {}", err)
                }
                _ => {}
            }
        }

        while self.frame_timer < 0.0 {
            //advance frame
            if self.compressed {
                match self.loader_rx.try_recv() {
                    Ok(LoaderResponse::ImageLoaded(img, idx))
                        if img.width() > 0 && idx == self.next_image() =>
                    {
                        let mut canvas = self.canvas.lock().unwrap();
                        let image_src =
                            femtovg::ImageSource::try_from(&img).expect("bad image format?");
                        canvas
                            .update_image(self.image_buffer[0], image_src, 0, 0)
                            .expect("Failed to update image data");
                        self.current_image = self.next_image();
                        self.loader_tx
                            .send(LoaderRequest::LoadIamge(self.next_image()))
                            .expect("Animation loader closed");
                    }
                    Ok(LoaderResponse::Error(err)) => {
                        log::warn!("Failed to load animation frame: {}", err)
                    }
                    _ => {}
                }
            } else {
                self.current_image += 1;
                self.current_image %= self.image_count;
            }
            self.frame_timer += self.frame_time;
        }
    }

    pub fn current_img_id(&self) -> femtovg::ImageId {
        if self.compressed {
            self.image_buffer
                .front()
                .copied()
                .unwrap_or_else(|| ImageId(generational_arena::Index::from_raw_parts(0, 0)))
        } else {
            self.image_buffer[self.current_image.min(self.image_buffer.len() - 1)]
        }
    }

    fn next_image(&self) -> usize {
        (self.current_image + 1) % self.image_count
    }
}
