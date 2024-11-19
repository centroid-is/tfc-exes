use crate::devices::device_trait::Index;
use crate::devices::device_trait::WriteValueIndex;
use crate::devices::device_trait::{Device, DeviceInfo};
use async_trait::async_trait;
use atomic_refcell::AtomicRefMut;
use ethercrab::{SubDevice, SubDevicePdi, SubDeviceRef};
use ethercrab_wire::EtherCrabWireRead;
use ethercrab_wire::EtherCrabWireWrite;
use log::warn;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::error::Error;
use tfc::confman::ConfMan;

use crate::devices::CiA402;

static RX_PDO_ASSIGN: u16 = 0x1C12;
static TX_PDO_ASSIGN: u16 = 0x1C13;
static RX_PDO_MAPPING: u16 = 0x1605;
static TX_PDO_MAPPING: u16 = 0x1A05;
static BASIC_MOTOR_CONTROL: u16 = 0x2631;

#[derive(ethercrab_wire::EtherCrabWireWrite, Debug)]
#[wire(bytes = 4)]
struct OutputPdo {
    #[wire(bits = 16)]
    control_word: CiA402::ControlWord,
    // control_word: ethercrab::ds402::ControlWord,
    #[wire(bits = 16)]
    speed: i16,
}

#[derive(ethercrab_wire::EtherCrabWireRead, Debug)]
#[wire(bytes = 6)]
struct InputPdo {
    #[wire(bits = 16)]
    status_word: CiA402::StatusWord,
    #[wire(bits = 16)]
    actual_speed: i16,
    #[wire(bits = 16)]
    error: i16,
}

#[derive(Debug, Copy, Clone, EtherCrabWireWrite, Serialize, Deserialize, JsonSchema)]
#[repr(u8)]
enum RatedMainsVoltage {
    Veff230 = 0,
    Veff400 = 1,
    Veff480 = 2,
    Veff120 = 3,
    Veff230ReducedLuLevel = 10,
}
impl Default for RatedMainsVoltage {
    fn default() -> Self {
        Self::Veff400
    }
}

impl Index for RatedMainsVoltage {
    const INDEX: u16 = 0x2540;
    const SUBINDEX: u8 = 0x01;
}

#[derive(Debug, EtherCrabWireWrite, Serialize, Deserialize, JsonSchema, Copy, Clone)]
#[wire(bytes = 2)]
#[serde(transparent)]
struct BaseVoltage {
    #[wire(bits = 16)]
    #[schemars(description = "Base voltage in volts")]
    value: u16,
}
impl Default for BaseVoltage {
    fn default() -> Self {
        Self { value: 400 }
    }
}
impl Index for BaseVoltage {
    const INDEX: u16 = 0x2B01;
    const SUBINDEX: u8 = 1;
}

#[derive(Debug, EtherCrabWireWrite, Serialize, Deserialize, JsonSchema, Copy, Clone)]
#[wire(bytes = 2)]
#[serde(transparent)]
struct BaseFrequency {
    #[wire(bits = 16)]
    value: u16,
}
impl Default for BaseFrequency {
    fn default() -> Self {
        Self { value: 50 }
    }
}
impl Index for BaseFrequency {
    const INDEX: u16 = 0x2B01;
    const SUBINDEX: u8 = 2;
}

#[derive(Debug, EtherCrabWireWrite, Serialize, Deserialize, JsonSchema, Copy, Clone)]
#[wire(bytes = 4)]
#[serde(transparent)]
struct MaxSpeed {
    #[wire(bits = 32)]
    value: u32,
}
impl Default for MaxSpeed {
    fn default() -> Self {
        Self { value: 6075 }
    }
}
impl Index for MaxSpeed {
    const INDEX: u16 = 0x6080;
    const SUBINDEX: u8 = 0;
}

#[derive(Debug, EtherCrabWireWrite, Serialize, Deserialize, JsonSchema, Copy, Clone)]
#[wire(bytes = 4)]
#[serde(transparent)]
struct MinSpeed {
    #[wire(bits = 32)]
    value: u32,
}
impl Default for MinSpeed {
    fn default() -> Self {
        Self { value: 0 }
    }
}
impl Index for MinSpeed {
    const INDEX: u16 = 0x6046;
    const SUBINDEX: u8 = 1;
}

#[derive(Debug, EtherCrabWireWrite, Serialize, Deserialize, JsonSchema, Copy, Clone)]
#[wire(bytes = 4)]
#[serde(transparent)]
struct AccelerationNumerator {
    #[wire(bits = 32)]
    value: u32, // rpm
}
impl Default for AccelerationNumerator {
    fn default() -> Self {
        Self { value: 3000 }
    }
}
impl Index for AccelerationNumerator {
    const INDEX: u16 = 0x6048;
    const SUBINDEX: u8 = 1;
}

#[derive(Debug, EtherCrabWireWrite, Serialize, Deserialize, JsonSchema, Copy, Clone)]
#[wire(bytes = 2)]
#[serde(transparent)]
struct AccelerationDenominator {
    #[wire(bits = 16)]
    value: u16, // seconds
}
impl Default for AccelerationDenominator {
    fn default() -> Self {
        Self { value: 10 }
    }
}
impl Index for AccelerationDenominator {
    const INDEX: u16 = 0x6048;
    const SUBINDEX: u8 = 2;
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Default)]
struct Acceleration {
    #[schemars(
        description = "Acceleration numerator in RPM",
        range(min = 0, max = 2147483647)
    )]
    numerator: AccelerationNumerator,
    #[schemars(
        description = "Acceleration denominator in seconds",
        range(min = 0, max = 65535)
    )]
    denominator: AccelerationDenominator,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Default)]
struct Config {
    // lenze i550 manual 5.8.2 Manual setting of the motor data
    // todo: add the other parameters
    rated_mains_voltage: RatedMainsVoltage,
    #[schemars(description = "Base voltage in volts", range(min = 0, max = 5000))]
    base_voltage: BaseVoltage,
    #[schemars(description = "Base frequency in hertz", range(min = 0, max = 1500))]
    base_frequency: BaseFrequency,
    #[schemars(description = "Max speed in RPM", range(min = 0, max = 480000))]
    max_speed: MaxSpeed,
    #[schemars(description = "Min speed in RPM", range(min = 0, max = 480000))]
    min_speed: MinSpeed,
    #[schemars(
        description = "Acceleration in deciseconds",
        range(min = 0, max = 36000)
    )]
    acceleration: Acceleration,
}

pub struct I550 {
    cnt: u128,
    config: ConfMan<Config>,
}

impl I550 {
    pub fn new(dbus: zbus::Connection, slave_number: u16, alias_address: u16) -> Self {
        Self {
            cnt: 0,
            config: ConfMan::new(
                dbus,
                &format!("i550_slave_{slave_number}_alias_{alias_address}"),
            ),
        }
    }
}

#[async_trait]
impl Device for I550 {
    async fn setup<'maindevice, 'group>(
        &mut self,
        device: &mut SubDeviceRef<'maindevice, AtomicRefMut<'group, SubDevice>>,
    ) -> Result<(), Box<dyn Error>> {
        warn!("Setting up I550");

        // reset fault
        device.sdo_write(BASIC_MOTOR_CONTROL, 4, 1 as u8).await?;

        device.sdo_write(RX_PDO_ASSIGN, 0x00, 0 as u8).await?;
        device.sdo_write(TX_PDO_ASSIGN, 0x00, 0 as u8).await?;

        // zero the size
        device.sdo_write(RX_PDO_MAPPING, 0x00, 0 as u8).await?;

        device
            .sdo_write(RX_PDO_MAPPING, 0x01, 0x60400010 as u32)
            .await?; // CMD - Control Word
        device
            .sdo_write(RX_PDO_MAPPING, 0x02, 0x60420010 as u32)
            .await?; // set speed

        device.sdo_write(RX_PDO_MAPPING, 0x00, 2 as u8).await?;

        // // // sdo_write<uint32_t>(ecx::tx_pdo_mapping<0x03>, 0x20020510);  // LCR  - CURRENT USAGE ( A
        // // // sdo_write<uint32_t>(ecx::tx_pdo_mapping<0x04>, 0x20160310);  // 1LIR - DI1-DI6
        // // // sdo_write<uint32_t>(ecx::tx_pdo_mapping<0x05>, 0x20291610);  // LFT  - Last error occured
        // // // sdo_write<uint32_t>(ecx::tx_pdo_mapping<0x06>, 0x20022910);  // HMIS - Drive state
        // // sdo_write<uint32_t>(
        // //     ecx::rx_pdo_mapping<0x03>,
        // //     0x20160D10);  // OL1R - Logic outputs states ( bit0: Relay 1, bit1: Relay 2, bit3 - bit7: unknown, bit8: DQ1 )
        // // sdo_write<uint32_t>(ecx::rx_pdo_mapping<0x04>, 0x203C0210);  // ACC - Acceleration
        // // sdo_write<uint32_t>(ecx::rx_pdo_mapping<0x05>, 0x203C0310);  // DEC - Deceleration

        // zero the size
        device.sdo_write(TX_PDO_MAPPING, 0x00, 0 as u8).await?;

        device
            .sdo_write(TX_PDO_MAPPING, 0x01, 0x60410010 as u32)
            .await?; // ETA  - STATUS WORD
        device
            .sdo_write(TX_PDO_MAPPING, 0x02, 0x60440010 as u32)
            .await?; // Actual speed
        device
            .sdo_write(TX_PDO_MAPPING, 0x03, 0x603F0010 as u32)
            .await?; // Error

        // Set tx size
        device.sdo_write(TX_PDO_MAPPING, 0x00, 3 as u8).await?;

        // Assign pdo's to mappings
        device
            .sdo_write(RX_PDO_ASSIGN, 0x01, RX_PDO_MAPPING as u16)
            .await?;
        device.sdo_write(RX_PDO_ASSIGN, 0x00, 1 as u8).await?;

        device
            .sdo_write(TX_PDO_ASSIGN, 0x01, TX_PDO_MAPPING as u16)
            .await?;
        device.sdo_write(TX_PDO_ASSIGN, 0x00, 1 as u8).await?;

        // cia 402 velocity mode
        device.sdo_write(0x6060, 0, 2 as u8).await?;

        device.sdo_write(BASIC_MOTOR_CONTROL, 0x01, 1 as u8).await?; // Set enable inverter to true

        // device.sdo_write(BASIC_MOTOR_CONTROL, 0x02, 1 as u8).await?; // Set allow run to constant true

        device
            .sdo_write_value_index(self.config.read().rated_mains_voltage)
            .await?;
        device
            .sdo_write_value_index(self.config.read().base_voltage)
            .await?;
        device
            .sdo_write_value_index(self.config.read().base_frequency)
            .await?;
        device
            .sdo_write_value_index(self.config.read().max_speed)
            .await?;
        // I don't know why max speed is in two different places in i550
        device
            .sdo_write(0x6046, 2, self.config.read().max_speed.value)
            .await?;
        device
            .sdo_write_value_index(self.config.read().min_speed)
            .await?;
        device
            .sdo_write_value_index(self.config.read().acceleration.numerator)
            .await?;
        device
            .sdo_write_value_index(self.config.read().acceleration.denominator)
            .await?;

        // device.sdo_write(0x6048, 1, 3000 as u32).await?;
        // device.sdo_write(0x6048, 2, 1 as u16).await?;
        warn!("I550 setup complete");
        Ok(())
    }
    async fn process_data<'maindevice, 'group>(
        &mut self,
        device: &mut SubDeviceRef<'maindevice, SubDevicePdi<'group>>,
    ) -> Result<(), Box<dyn Error>> {
        let (input, output) = device.io_raw_mut();

        if output.len() != 4 {
            warn!("Output PDO length is not 4");
            return Ok(());
        }

        if input.len() != 6 {
            warn!("Input PDO length is not 6");
            return Ok(());
        }

        let input_pdo = InputPdo::unpack_from_slice(&input).expect("Error unpacking input PDO");

        let control_word = CiA402::transition(
            input_pdo.status_word.parse_state(),
            CiA402::TransitionAction::Run,
            true,
        );
        let output_pdo = OutputPdo {
            control_word,
            speed: 1000,
        };
        output_pdo
            .pack_to_slice(&mut *output)
            .expect("Error packing output PDO");

        self.cnt += 1;

        if self.cnt % 1000 == 0 {
            warn!("output_pdo: {:?}", output_pdo);
            warn!("input_pdo: {:?}", input_pdo);
            warn!("output: {:?}", output);
        }

        Ok(())
    }
}

impl DeviceInfo for I550 {
    const VENDOR_ID: u32 = 0x0000003b;
    const PRODUCT_ID: u32 = 0x69055000;
    const NAME: &'static str = "i550";
}

#[cfg(test)]
mod tests {
    use super::*;

    use uom::si::electric_potential::decivolt;
    use uom::si::length::centimeter;
    // mod cgs {
    //     use uom::system;
    //     uom::ISQ!(
    //         uom::si,
    //         f32,
    //         (centimeter, gram, second, ampere, kelvin, mole, candela)
    //     );
    // }
    // mod foo {
    //     use uom::system;
    //     uom::ISQ!(uom::si, i16, (decivolt));
    // }

    #[test]
    fn test_base_voltage() {
        // assert_eq!(base_voltage.value, 4000);

        // // store base voltage quantity in decivolt
        let base_voltage =
            uom::si::i16::ElectricPotential::new::<uom::si::electric_potential::decivolt>(4001);
        let value_in_decivolts = base_voltage.get::<uom::si::electric_potential::decivolt>();
        assert_eq!(value_in_decivolts, 4001);
        assert_eq!(base_voltage.value, 4000);
    }
}
