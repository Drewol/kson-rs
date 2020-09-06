pub struct Action<T> {
    pub description: String,
    pub action: Box<dyn Fn(&mut T) -> Result<(), String>>,
}
pub struct ActionStack<T: Clone> {
    original: T,
    undo_stack: Vec<Action<T>>,
    redo_stack: Vec<Action<T>>,
    saved: bool,
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
            saved: true,
        }
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

    pub fn commit(&mut self, action: Action<T>) {
        self.undo_stack.push(action);
        self.redo_stack.clear();
    }

    pub fn reset(&mut self, origin: T) {
        self.original = origin;
        self.redo_stack.clear();
        self.undo_stack.clear();
    }

    pub fn apply(&mut self) {
        if let Ok(current) = self.get_current() {
            self.reset(current);
        }
    }

    pub fn next_action_desc(&self) -> Option<String> {
        if let Some(next) = self.redo_stack.last() {
            Some(next.description.clone())
        } else {
            None
        }
    }

    pub fn prev_action_desc(&self) -> Option<String> {
        if let Some(next) = self.undo_stack.last() {
            Some(next.description.clone())
        } else {
            None
        }
    }

    pub fn get_current(&mut self) -> Result<T, String> {
        let mut current = self.original.clone();
        for action in &self.undo_stack {
            action.action.as_ref()(&mut current)?;
        }
        Ok(current)
    }

    pub fn set_saved(&mut self, saved: bool) {
        self.saved = saved;
    }
}
