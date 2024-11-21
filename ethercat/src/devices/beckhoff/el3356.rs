use async_trait::async_trait;
use atomic_refcell::AtomicRefMut;
use ethercrab::{SubDevice, SubDevicePdi, SubDeviceRef};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWrite};
use std::error::Error;

use crate::devices::device_trait::{Device, DeviceInfo};

#[derive(Debug, EtherCrabWireRead)]
#[wire(bytes = 2)]
struct StatusWord {
    #[wire(bits = 1, pre_skip = 1)]
    over_range: bool,
    #[wire(bits = 1, pre_skip = 1)]
    data_invalid: bool,
    #[wire(bits = 1, pre_skip = 2)]
    collective_error: bool,
    #[wire(bits = 1)]
    calibration_in_progress: bool,
    #[wire(bits = 1)]
    /// Steady state (Idling recognition)
    /// If the load value remains within a range of values y for longer than time x, then the SteadyState is
    /// activated in the StatusWord.
    /// TODO: The parameters x and y can be specified in the CoE
    steady_state: bool,
    #[wire(bits = 1, pre_skip = 4)]
    /// Synchronization error
    sync_error: bool,
    #[wire(bits = 1, pre_skip = 1)]
    /// toggeles 0->1->0 with each updated data set
    tx_pdo: bool,
}

#[derive(Debug, EtherCrabWireWrite)]
#[wire(bytes = 2)]
struct ControlWord {
    #[wire(bits = 1)]
    /// Starts the self-calibration immediately
    start_calibration: bool,
    #[wire(bits = 1)]
    /// The measuring amplifiers are periodically subjected to examination and self-calibration. Several analog
    /// switches are provided for this purpose, so that the various calibration signals can be connected. It is
    /// important for this process that the entire signal path, including all passive components, is examined at every
    /// phase of the calibration. Only the interference suppression elements (L/C combination) and the analog
    /// switches themselves cannot be examined. In addition, a self-test is carried out at longer intervals.
    /// The self-calibration is carried out every three minutes in the default setting.
    /// Self-calibration
    /// The time interval is set in 100 ms steps with object 0x8000:31 [} 174]; default: 3 min.
    /// Duration approx. 150 ms
    /// Self-test
    /// is additional carried out together with every nth self-calibration.
    /// The multiple (n) is set with object 0x8000:32 [} 174]; default: 10
    /// additional duration approx. 70 ms.
    disable_calibration: bool,
    #[wire(bits = 1)]
    /// If the terminal is placed in the freeze state by InputFreeze in the control word, no further analog measured
    /// values are relayed to the internal filter. This function is usable, for example, if a filling surge is expected from
    /// the application that would unnecessarily overdrive the filters due to the force load. This would result in a
    /// certain amount of time elapsing until the filter had settled again. The user himself must determine a sensible
    /// InputFreeze time for his filling procedure.
    input_freeze: bool,
    #[wire(bits = 1)]
    /// Mode 0: High precision Analog conversion at 10.5 kSps (samples per second) Slow conversion and thus high accuracy
    /// Typical Latency 7.2 ms
    /// Mode 1: High speed / low latency Analog conversion at 105.5 kSps (samples per second) Fast conversion with low latency
    /// Typical Latency 0.72 ms
    sample_mode: bool,
    #[wire(bits = 1, post_skip = 11)]
    /// When taring, the scales are set to zero using an arbitrary applied load; i.e. an offset correction is performed.
    /// The EL3356 supports two tarings; it is recommended to set a strong filter when taring.
    /// Temporary tare: The correction value is NOT stored in the terminal and is lost in the event of a power failure.
    /// Permanent tare: The correction value is stored locally in the terminal's EEPROM and is not lost in the event of a power
    /// failure.
    tare: bool,
}

#[derive(Debug, EtherCrabWireRead)]
#[wire(bytes = 6)]
struct InputPdo {
    #[wire(bits = 16)]
    value: StatusWord,
    #[wire(bits = 32)]
    raw_value: i32,
}

#[derive(Debug, EtherCrabWireWrite)]
#[wire(bytes = 2)]
struct OutputPdo {
    #[wire(bits = 16)]
    control_word: ControlWord,
}

pub struct El3356 {
    cnt: u128,
}

impl El3356 {
    pub fn new(dbus: zbus::Connection, slave_number: u16, alias_address: u16) -> Self {
        Self { cnt: 0 }
    }
}

#[async_trait]
impl Device for El3356 {
    async fn setup<'maindevice, 'group>(
        &mut self,
        device: &mut SubDeviceRef<'maindevice, AtomicRefMut<'group, SubDevice>>,
    ) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    async fn process_data<'maindevice, 'group>(
        &mut self,
        device: &mut SubDeviceRef<'maindevice, SubDevicePdi<'group>>,
    ) -> Result<(), Box<dyn Error>> {
        self.cnt += 1;
        let (i, mut o) = device.io_raw_mut();
        if self.cnt % 1000 == 0 {
            let input_pdo = InputPdo::unpack_from_slice(&i).expect("Error unpacking input PDO");
            let output_pdo = OutputPdo {
                control_word: ControlWord {
                    start_calibration: false,
                    disable_calibration: true,
                    input_freeze: false,
                    sample_mode: false,
                    tare: false,
                },
            };
            println!("El3356: {}, i: {input_pdo:?}, o: {o:?}", self.cnt);
            output_pdo
                .pack_to_slice(&mut o)
                .expect("Error packing output PDO");
        }
        Ok(())
    }
}

impl DeviceInfo for El3356 {
    const VENDOR_ID: u32 = 0x2;
    const PRODUCT_ID: u32 = 0x0d1c3052;
    const NAME: &'static str = "El3356";
}
