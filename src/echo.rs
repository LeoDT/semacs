#[derive(Default)]
pub struct EchoArea {
    message: String,
}

impl EchoArea {
    pub fn set(&mut self, message: impl Into<String>) {
        self.message = message.into();
    }

    pub fn clear(&mut self) {
        self.message.clear();
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}
