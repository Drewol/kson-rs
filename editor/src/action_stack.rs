use anyhow::Result;

type ActionFn<T> = Box<dyn Fn(&mut T) -> Result<()>>;
pub struct Action<T> {
    id: u32,
    pub description: String,
    pub action: ActionFn<T>,
}

pub struct ActionStack<T: Clone> {
    original: T,
    undo_stack: Vec<Action<T>>,
    redo_stack: Vec<Action<T>>,
    saved: Option<u32>,
    next_id: u32,
}

impl<T> ActionStack<T>
where
    T: Clone,
{
    pub fn new(original: T) -> Self {
        ActionStack {
            original,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            saved: None,
            next_id: 0,
        }
    }

    pub fn new_action(&mut self) -> &mut Action<T> {
        self.undo_stack.push(Action {
            action: Box::new(|_| panic!("Unset Action")),
            description: String::new(),
            id: self.next_id,
        });
        self.next_id += 1;
        self.redo_stack.clear();
        self.undo_stack.last_mut().unwrap()
    }

    pub fn undo(&mut self) {
        if let Some(action) = self.undo_stack.pop() {
            self.redo_stack.push(action);
        }
    }

    pub fn redo(&mut self) {
        if let Some(action) = self.redo_stack.pop() {
            self.undo_stack.push(action);
        }
    }

    pub fn reset(&mut self, origin: T) {
        self.original = origin;
        self.redo_stack.clear();
        self.undo_stack.clear();
        self.saved = None;
    }

    #[allow(unused)]
    pub fn apply(&mut self) {
        if let Ok(current) = self.get_current() {
            self.reset(current);
        }
    }

    pub fn next_action_desc(&self) -> Option<String> {
        self.redo_stack.last().map(|next| next.description.clone())
    }

    pub fn prev_action_desc(&self) -> Option<String> {
        self.undo_stack.last().map(|next| next.description.clone())
    }

    pub fn get_current(&mut self) -> Result<T> {
        let mut current = self.original.clone();
        for action in &self.undo_stack {
            action.action.as_ref()(&mut current)?;
        }
        Ok(current)
    }

    pub fn save(&mut self) {
        match self.undo_stack.last() {
            Some(a) => self.saved = Some(a.id),
            None => self.saved = None,
        }
    }

    pub fn saved(&self) -> bool {
        match (self.undo_stack.last(), self.saved) {
            (Some(a), Some(saved)) => a.id == saved,
            (Some(_), None) => false,
            _ => true,
        }
    }
}
