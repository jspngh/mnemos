use common::{
    syscall::request::SysCallRequest,
    syscall::{success::{SysCallSuccess, TimeSuccess, BlockSuccess, BlockInfo}, request::BlockRequest},
    syscall::{
        request::{SerialRequest, TimeRequest},
        success::SerialSuccess,
    },
};
use groundhog::RollingTimer;
use groundhog_nrf52::GlobalRollingTimer;

pub trait Serial: Send {
    fn register_port(&mut self, port: u16) -> Result<(), ()>;
    fn release_port(&mut self, port: u16) -> Result<(), ()>;
    fn process(&mut self);

    // On success: The valid received part (<= buf.len()). Can be &[] (if no bytes)
    // On error: TODO
    fn recv<'a>(&mut self, port: u16, buf: &'a mut [u8]) -> Result<&'a mut [u8], ()>;

    // On success: All bytes were sent/enqueued.
    // On error: the portion of bytes that were NOT sent (the remainder). (<= buf.len()).
    // CANNOT be &[].
    fn send<'a>(&mut self, port: u16, buf: &'a [u8]) -> Result<(), &'a [u8]>;
}

pub trait BlockStorage: Send {
    fn block_count(&self) -> u32;
    fn block_size(&self) -> u32;
    fn block_info<'a>(&'a self, block: u32) -> Result<BlockInfo<'a>, ()>;
}

// pub trait SendSerial: Serial + Send {}

pub struct Machine {
    pub serial: &'static mut dyn Serial,
    pub block_storage: Option<&'static mut dyn BlockStorage>,
    // TODO: port router?
    // TODO: flash manager?
}

impl Machine {
    pub fn handle_syscall<'a>(
        &mut self,
        req: SysCallRequest<'a>,
    ) -> Result<SysCallSuccess<'a>, ()> {
        match req {
            SysCallRequest::Serial(SerialRequest::SerialReceive { port, dest_buf }) => {
                let dest_buf = unsafe { dest_buf.to_slice_mut() };
                let used = self.serial.recv(port, dest_buf)?;
                Ok(SysCallSuccess::Serial(SerialSuccess::DataReceived {
                    dest_buf: used.into(),
                }))
            }
            SysCallRequest::Serial(SerialRequest::SerialSend { port, src_buf }) => {
                let src_buf = unsafe { src_buf.to_slice() };
                match self.serial.send(port, src_buf) {
                    Ok(()) => Ok(SysCallSuccess::Serial(SerialSuccess::DataSent {
                        remainder: None,
                    })),
                    Err(rem) => Ok(SysCallSuccess::Serial(SerialSuccess::DataSent {
                        remainder: Some(rem.into()),
                    })),
                }
            }
            SysCallRequest::Serial(SerialRequest::SerialOpenPort { port }) => {
                self.serial.register_port(port)?;
                Ok(SysCallSuccess::Serial(SerialSuccess::PortOpened))
            }
            SysCallRequest::Time(TimeRequest::SleepMicros { us }) => {
                let timer = GlobalRollingTimer::default();
                let mut ttl_us = us;
                let orig_start = timer.get_ticks();

                // Just in case the user asks for something REALLY close to a rollover (e.g. u32::MAX),
                // don't delay for more than half of the range of the timer.
                while ttl_us != 0 {
                    let start = timer.get_ticks();
                    let to_wait = ttl_us.min(u32::MAX / 2);
                    while timer.micros_since(start) <= to_wait {}
                    ttl_us = ttl_us.saturating_sub(to_wait)
                }

                Ok(SysCallSuccess::Time(TimeSuccess::SleptMicros {
                    us: timer.micros_since(orig_start).min(us),
                }))
            }
            SysCallRequest::BlockStore(bsr) => {
                let resp = self.handle_block_request(bsr)?;
                Ok(SysCallSuccess::BlockStore(resp))
            },
        }
    }

    pub fn handle_block_request<'a>(
        &mut self,
        req: BlockRequest<'a>,
    ) -> Result<BlockSuccess<'a>, ()> {
        // Match early to provide the "null" storage info if we have none.
        let sto: &mut dyn BlockStorage = match (self.block_storage.as_mut(), &req) {
            (None, BlockRequest::StoreInfo) => {
                return Ok(BlockSuccess::StoreInfo {
                    blocks: 0,
                    capacity: 0,
                });
            },
            (None, _) => return Err(()),
            (Some(sto), _) => *sto,
        };

        match req {
            BlockRequest::StoreInfo => {
                Ok(BlockSuccess::StoreInfo {
                    blocks: sto.block_count(),
                    capacity: sto.block_size(),
                })
            },
            // BlockRequest::BlockInfo { block_idx, dest_buf } => todo!(),
            // BlockRequest::BlockOpen { block_idx } => todo!(),
            // BlockRequest::BlockRead { block_idx, offset, dest_buf } => todo!(),
            // BlockRequest::BlockWrite { block_idx, offset, src_buf } => todo!(),
            // BlockRequest::BlockClose { block_idx, name, len, kind } => todo!(),
            _ => {
                // TODO: All this stuff ^^
                defmt::println!("Oops, unsupported block command, my bad.");
                Err(())
            }
        }
    }
}
