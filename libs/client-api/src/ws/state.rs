#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ConnectState {
    PingTimeout,
    Connecting,
    Connected,
    Unauthorized,
    Closed,
}

impl ConnectState {
    #[allow(dead_code)]
    pub fn is_connecting(&self) -> bool {
        matches!(self, ConnectState::Connecting)
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, ConnectState::Connected)
    }

    #[allow(dead_code)]
    pub fn is_timeout(&self) -> bool {
        matches!(self, ConnectState::PingTimeout)
    }

    #[allow(dead_code)]
    pub fn is_closed(&self) -> bool {
        matches!(self, ConnectState::Closed)
    }
}
