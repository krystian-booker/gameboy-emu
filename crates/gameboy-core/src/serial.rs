#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Serial {
    data: u8,
    control: u8,
    output: Vec<u8>,
}

impl Serial {
    pub fn read_data(&self) -> u8 {
        self.data
    }

    pub fn write_data(&mut self, value: u8) {
        self.data = value;
    }

    pub fn read_control(&self) -> u8 {
        0x7E | self.control
    }

    pub fn write_control(&mut self, value: u8) -> bool {
        self.control = value & 0x81;
        let internal_clock_transfer = self.control & 0x81 == 0x81;

        if internal_clock_transfer {
            self.output.push(self.data);
            self.control &= !0x80;
        }

        internal_clock_transfer
    }

    pub fn drain_output(&mut self) -> Vec<u8> {
        self.output.drain(..).collect()
    }
}
