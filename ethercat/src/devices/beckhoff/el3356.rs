use async_trait::async_trait;
use atomic_refcell::AtomicRefMut;
use ethercrab::{EtherCrabWireReadWrite, SubDevice, SubDevicePdi, SubDeviceRef};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWrite};
use log::warn;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::error::Error;
use tfc::confman::ConfMan;

use crate::define_value_type;
use crate::devices::device_trait::{Device, DeviceInfo, Index, WriteValueIndex};

static RX_PDO_ASSIGN: u16 = 0x1C12;
static TX_PDO_ASSIGN: u16 = 0x1C13;
static RX_PDO_MAPPING: u16 = 0x1600;
static TX_PDO_MAPPING: u16 = 0x1A00;

smlang::statemachine! {
    name: Calibrate,
    derive_states: [Debug, Clone],
    derive_events: [Debug, Clone],
    transitions: {
        *Idle + SetZeroCalibration = ZeroCalibration,
        ZeroCalibration + SetCalibration = Calibration,
        Calibration + SetIdle = Idle,
    }
}

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

#[derive(Debug, EtherCrabWireReadWrite, Copy, Clone, Serialize, Deserialize, JsonSchema)]
#[repr(u16)]
enum Filter {
    FIR50 = 0,
    FIR60 = 1,
    IIR1 = 2,
    IIR2 = 3,
    IIR3 = 4,
    IIR4 = 5,
    IIR5 = 6,
    IIR6 = 7,
    IIR7 = 8,
    IIR8 = 9,
    DynamicIIR = 10,
    PDOFilterFrequency = 11,
}
impl Default for Filter {
    fn default() -> Self {
        Self::FIR50
    }
}
impl Index for Filter {
    const INDEX: u16 = 0x8000;
    const SUBINDEX: u8 = 0x11;
}

define_value_type!(NominalValue, f32, 1.0, 0x8000, 0x23);
define_value_type!(Gravity, f32, 9.806650, 0x8000, 0x26);
define_value_type!(ZeroBalance, f32, 0.0, 0x8000, 0x25);
define_value_type!(ScaleFactor, f32, 1000.0, 0x8000, 0x27);

#[derive(Serialize, Deserialize, JsonSchema, Debug)]
struct Config {
    filter: Filter,
    nominal_load: f32,     // kg
    calibration_load: f32, // kg
    #[schemars(
        description = "Nominal characteristic value of the sensor element, Set to 1 if you wan't raw value from load cell. mV/V"
    )]
    nominal_value: Option<NominalValue>,
    #[schemars(
        description = "Gravity of earth, default: None is 9.80665 m/s^2. Set to 1 if you want raw value from load cell"
    )]
    gravity: Option<Gravity>,
    #[schemars(
        description = "Zero balance of the sensor element. mV/V. Set to 0 if you want raw value from load cell"
    )]
    zero_balance: Option<ZeroBalance>,
    #[schemars(
        description = "This factor can be used to re-scale the process data. In order to change the display from kg to g, for example, the factor 1000 can be entered here."
    )]
    scale_factor: Option<ScaleFactor>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            filter: Filter::default(),
            nominal_load: 5.0,
            calibration_load: 5.0,
            nominal_value: None,
            gravity: None,
            zero_balance: None,
            scale_factor: None,
        }
    }
}

pub struct El3356 {
    cnt: u128,
    config: ConfMan<Config>,
    log_key: String,
}

impl El3356 {
    pub fn new(dbus: zbus::Connection, slave_number: u16, alias_address: u16) -> Self {
        let mut prefix = format!("el3356_{slave_number}");
        if alias_address != 0 {
            prefix = format!("el3356_alias_{alias_address}");
        }
        Self {
            cnt: 0,
            config: ConfMan::new(dbus.clone(), &prefix),
            log_key: prefix,
        }
    }
    pub async fn zero_calibrate<S: std::ops::Deref<Target = SubDevice>>(
        device: &mut SubDeviceRef<'_, S>,
        nominal_load: f32,
    ) -> Result<(), ethercrab::error::Error> {
        // 1. Perform a CoE reset with object 0x1011:01 see Restoring the delivery state [} 206
        // If this object is set to “0x64616F6C” in the set value dialog, all backup objects are reset to their delivery state.
        device.sdo_write(0x1011, 0x01, 0x64616F6C as u32).await?;
        // 2. Activate mode 0 via the control word (EL3356-0010 only)
        // JBB NOTE: We only use mode 0 which is high precision
        // 3. Set scale factor to 1 (0x8000:27 [} 174])
        device.sdo_write(0x8000, 0x27, 1 as f32).await?;
        // 4. Set gravity of earth (0x8000:26) [} 174] if necessary (default: 9.806650)
        // JBB NOTE: We don't need to change this maybe later
        // 5. Set gain to (0x8000:21 [} 174]) = 1
        device.sdo_write(0x8000, 0x21, 1 as f32).await?;
        // 6. Set tare to 0 (0x8000:22 [} 174])
        device.sdo_write(0x8000, 0x22, 0 as f32).await?;
        // 7. Set the filter (0x8000:11 [} 174]) to the strongest level: IIR8
        device.sdo_write_value_index(Filter::IIR8).await?;
        // 8. Specify the nominal load of the sensor in 0x8000:24 [} 174] (“Nominal load”)
        // JBB NOTE: I disagree with this, why is it needed to know nominal load? But let's do it
        device.sdo_write(0x8000, 0x24, nominal_load).await?;
        // 9. Zero balance: Do not load the scales
        // As soon as the measured value indicates a constant value for at least 10 seconds, execute the
        // command “0x0101” (257dec) on CoE object 0xFB00:01 [} 176].
        // This command causes the current mV/V value (0x9000:11 [} 177]) to be entered in the “Zero balance” object.
        // Check: CoE objects 0xFB00:02 and 0xFB00:03 must contain “0” after execution.
        device.sdo_write(0xFB00, 0x01, 0x0101 as u16).await?; // todo this is of type OCTET - STRING[2] ?
        loop {
            let status: u8 = device.sdo_read(0xFB00, 0x02).await?;
            let response: u32 = device.sdo_read(0xFB00, 0x03).await?;
            if status == 0 && response == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        Ok(())
    }
    pub async fn calibrate<S: std::ops::Deref<Target = SubDevice>>(
        device: &mut SubDeviceRef<'_, S>,
        calibration_load: f32,
        filter: Filter,
    ) -> Result<(), ethercrab::error::Error> {
        // 10. Load the scales with a reference load. This should be at least 20% of the rated load. The larger the
        // reference load, the better the sensor values can be calculated.
        // In object 0x8000:28 [} 174] (“Reference load”), enter the load in the same unit as the rated load (0x8000:24 [} 174]).
        // As soon as the measured value indicates a constant value for at least 10 seconds, execute the
        // command “0x0102” (258dec) on CoE object 0xFB00:01 [} 176].
        // By means of this command the EL3356 determines the output value for the nominal weight (“Rated output”)
        // Check: CoE objects 0xFB00:02 and 0xFB00:03 must contain “0” after execution.
        device.sdo_write(0x8000, 0x28, calibration_load).await?;
        device.sdo_write(0xFB00, 0x01, 0x0102 as u16).await?; // todo this is of type OCTET - STRING[2] ?
        loop {
            let status: u8 = device.sdo_read(0xFB00, 0x02).await?;
            let response: u32 = device.sdo_read(0xFB00, 0x03).await?;
            if status == 0 && response == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        // 11. Reset: execute the command “0x0000” (0dec) on CoE object 0xFB00:01 [} 176].
        device.sdo_write(0xFB00, 0x01, 0x0000 as u16).await?;
        // 12. Set the filter to a lower stage.
        device.sdo_write_value_index(filter).await?;
        Ok(())
    }
}

#[async_trait]
impl Device for El3356 {
    async fn setup<'maindevice, 'group>(
        &mut self,
        device: &mut SubDeviceRef<'maindevice, AtomicRefMut<'group, SubDevice>>,
    ) -> Result<(), Box<dyn Error>> {
        // device.sdo_write(RX_PDO_ASSIGN, 0x00, 0 as u8).await?;
        // device.sdo_write(TX_PDO_ASSIGN, 0x00, 0 as u8).await?;

        // // zero the size
        // device.sdo_write(RX_PDO_MAPPING, 0x00, 0 as u8).await?;

        // // address 0x60FD, subindex 0x00, length 0x20 = 32 bytes
        // device
        //     .sdo_write(RX_PDO_MAPPING, 0x01, 0x70000101 as u32)
        //     .await?; // start calibration

        // device
        //     .sdo_write(RX_PDO_MAPPING, 0x02, 0x70000201 as u32)
        //     .await?; // disable calibration

        // device
        //     .sdo_write(RX_PDO_MAPPING, 0x03, 0x70000301 as u32)
        //     .await?; // input freeze

        // device
        //     .sdo_write(RX_PDO_MAPPING, 0x04, 0x70000401 as u32)
        //     .await?; // sample mode

        // device
        //     .sdo_write(RX_PDO_MAPPING, 0x05, 0x70000501 as u32)
        //     .await?; // tara

        // device
        //     .sdo_write(RX_PDO_MAPPING, 0x06, 0x00000003 as u32)
        //     .await?; // 3 bits alignment

        // device
        //     .sdo_write(RX_PDO_MAPPING, 0x07, 0x00000008 as u32)
        //     .await?; // 8 bits alignment

        // device.sdo_write(RX_PDO_MAPPING, 0x00, 8 as u8).await?;

        // // zero the size
        // device.sdo_write(TX_PDO_MAPPING, 0x00, 0 as u8).await?;

        if let Some(nominal_value) = self.config.read().nominal_value {
            device.sdo_write_value_index(nominal_value).await?;
        }
        if let Some(gravity) = self.config.read().gravity {
            device.sdo_write_value_index(gravity).await?;
        }
        if let Some(zero_balance) = self.config.read().zero_balance {
            device.sdo_write_value_index(zero_balance).await?;
        }
        device.sdo_write(0x8000, 0x27, 1000000.0 as f32).await?;

        device.sdo_write(TX_PDO_ASSIGN, 0x00, 0 as u8).await?;
        device.sdo_write(TX_PDO_ASSIGN, 0x01, 0x1A00 as u16).await?;
        // device.sdo_write(TX_PDO_ASSIGN, 0x02, 0x1A02 as u16).await?; // use REAL from 0x1A02 pdo mapping
        device.sdo_write(TX_PDO_ASSIGN, 0x02, 0x1A01 as u16).await?; // use int from 0x1A02 pdo mapping
        device.sdo_write(TX_PDO_ASSIGN, 0x00, 0x02 as u8).await?;

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
            warn!(target: &self.log_key, "El3356: {}, i: {input_pdo:?}, o: {o:?}", self.cnt);
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
