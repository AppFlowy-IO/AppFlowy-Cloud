use std::time::{Duration, SystemTime};

const EPOCH: u64 = 1420070400000;
const NODE_ID_BITS: u64 = 10;
const SEQUENCE_BITS: u64 = 12;
const NODE_ID_SHIFT: u64 = SEQUENCE_BITS;
const TIMESTAMP_SHIFT: u64 = NODE_ID_BITS + SEQUENCE_BITS;
const SEQUENCE_MASK: u64 = (1 << SEQUENCE_BITS) - 1;
const MAX_NODE_ID: u64 = (1 << NODE_ID_BITS) - 1;

struct Snowflake {
    node_id: u64,
    sequence: u64,
    last_timestamp: u64,
}

impl Snowflake {
    pub fn new(node_id: u64) -> Snowflake {
        Snowflake {
            node_id,
            sequence: 0,
            last_timestamp: 0,
        }
    }

    pub fn next_id(&mut self) -> u64 {
        let timestamp = self.timestamp();
        if timestamp < self.last_timestamp {
            panic!("Clock moved backwards!");
        }

        if timestamp == self.last_timestamp {
            self.sequence = (self.sequence + 1) & SEQUENCE_MASK;
            if self.sequence == 0 {
                self.wait_next_millis();
            }
        } else {
            self.sequence = 0;
        }

        self.last_timestamp = timestamp;
        (timestamp - EPOCH) << TIMESTAMP_SHIFT | self.node_id << NODE_ID_SHIFT | self.sequence
    }

    fn wait_next_millis(&self) {
        let mut timestamp = self.timestamp();
        while timestamp == self.last_timestamp {
            timestamp = self.timestamp();
        }
    }

    fn timestamp(&self) -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }
}

fn main() {
    let mut snowflake = Snowflake::new(1);
    println!("{}", snowflake.next_id());
}
