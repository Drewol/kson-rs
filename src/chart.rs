pub extern crate serde_json;
pub extern crate serde;

use self::serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
pub struct GraphSectionPoint {
    ry : u32,
    v : f64,
    vf : f64,
    a : f64,
    b : f64
}

#[derive(Serialize, Deserialize)]
pub struct Interval {
    y : u32,
    l : u32
}

#[derive(Serialize, Deserialize)]
pub struct LaserSection {
    y : u32,
    v : Vec<GraphSectionPoint>
}

#[derive(Serialize, Deserialize)]
pub struct NoteInfo {
    bt : [Vec<Interval>; 4],
    fx : [Vec<Interval>; 2],
    laser : [Vec<LaserSection>; 2]
}

impl NoteInfo {
    fn new() -> NoteInfo {
        NoteInfo {
            bt : [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            fx : [Vec::new(), Vec::new()],
            laser : [Vec::new(), Vec::new()]
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct DifficultyInfo {
    name : String,
    short_name : String,
    idx : u8
}

#[derive(Serialize, Deserialize)]
pub struct MetaInfo {
    title : String,
    title_translit : String,
    subtitle : String,
    artist : String,
    artist_translit : String,
    chart_author : String,
    difficulty : DifficultyInfo,
    level : u8,
    disp_bpm : String,
    std_bpm : f64,
    jacket_filename : String,
    jacket_author : String,
    information : String
}

impl DifficultyInfo {
    fn new() -> DifficultyInfo {
        DifficultyInfo {
            name : String::new(),
            short_name : String::new(),
            idx : 0
        }
    }
}

impl MetaInfo {
    fn new() -> MetaInfo
    {
         MetaInfo {
                title : String::new(),
                title_translit : String::new(),
                subtitle : String::new(),
                artist : String::new(),
                artist_translit : String::new(),
                chart_author : String::new(),
                difficulty : DifficultyInfo::new(),
                level : 1,
                disp_bpm : String::new(),
                std_bpm : std::f64::NAN,
                jacket_filename : String::new(),
                jacket_author : String::new(),
                information : String::new()
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Chart {
    note_info : NoteInfo,
    meta : MetaInfo
}

impl Chart {
    pub fn new() -> Chart {
        Chart
        {
            meta : MetaInfo::new(),
            note_info : NoteInfo::new()
        }
    }
}