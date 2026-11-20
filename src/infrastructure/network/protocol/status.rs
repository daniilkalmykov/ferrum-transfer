pub enum Status {
    Ok,
    Ready,
    Error,
}

impl From<Status> for u8 {
    fn from(status: Status) -> Self {
        match status {
            Status::Ok => 0,
            Status::Ready => 1,
            Status::Error => 2,
        }
    }
}
