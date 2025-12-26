pub struct LastModifiedAt {
    pub last_modified_at: u128,
}

impl LastModifiedAt {
    pub fn new() -> Self {
        Self {last_modified_at: 0}
    }

    pub fn update(&mut self, last_modified_at: u128) {
        self.last_modified_at = self.last_modified_at.max(last_modified_at);
    }

    pub fn has_changed_since(&self, time: u128) -> bool {
        self.last_modified_at > time
    }

    pub fn as_nanos(&self) -> u128 {
        self.last_modified_at
    }
}
