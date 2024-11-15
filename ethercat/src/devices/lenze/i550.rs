use crate::devices::device_trait::{Device, DeviceInfo};
use async_trait::async_trait;
use atomic_refcell::AtomicRefMut;
use ethercrab::ds402::ControlWord;
use ethercrab::ds402::StatusWord;
use ethercrab::{SubDevice, SubDevicePdi, SubDeviceRef};
use ethercrab_wire::EtherCrabWireRead;
use ethercrab_wire::EtherCrabWireWrite;
use log::warn;
use std::error::Error;

pub struct I550 {
    cnt: u128,
}

impl I550 {
    pub fn new(dbus: zbus::Connection, slave_number: u16, alias_address: u16) -> Self {
        Self { cnt: 0 }
    }
}

static RX_PDO_ASSIGN: u16 = 0x1C12;
static TX_PDO_ASSIGN: u16 = 0x1C13;
static RX_PDO_MAPPING: u16 = 0x1605;
static TX_PDO_MAPPING: u16 = 0x1A05;
static BASIC_MOTOR_CONTROL: u16 = 0x2631;

#[derive(ethercrab_wire::EtherCrabWireReadWrite, Debug)]
#[wire(bytes = 4)]
struct OutputPdo {
    #[wire(bits = 16)]
    control_word: u16,
    // control_word: ethercrab::ds402::ControlWord,
    #[wire(bits = 16)]
    speed: i16,
}

#[derive(ethercrab_wire::EtherCrabWireReadWrite, Debug)]
#[wire(bytes = 6)]
struct InputPdo {
    #[wire(bits = 16)]
    status_word: u16,
    #[wire(bits = 16)]
    actual_speed: i16,
    #[wire(bits = 16)]
    error: i16,
}

#[derive(Debug, PartialEq)]
#[repr(u16)]
enum DriveState {
    NotReadyToSwitchOn = 1,
    SwitchOnDisabled = 2,
    ReadyToSwitchOn = 3,
    SwitchedOn = 4,
    OperationEnabled = 5,
    QuickStopActive = 6,
    FaultReactionActive = 7,
    Fault = 8,
}

/// Parse the drive state from status word bits
///
/// Status Word Bit Mapping:
/// - Bit 0: Ready to switch on
/// - Bit 1: Switched on
/// - Bit 2: Operation enabled
/// - Bit 3: Fault
/// - Bit 4: Voltage enabled
/// - Bit 5: Quick stop (enabled low)
/// - Bit 6: Switch on disabled
fn parse_state(status: StatusWord) -> DriveState {
    let state_ready_to_switch_on = status.contains(StatusWord::READY_TO_SWITCH_ON);
    let state_switched_on = status.contains(StatusWord::SWITCHED_ON);
    let state_operation_enabled = status.contains(StatusWord::OP_ENABLED);
    let state_fault = status.contains(StatusWord::FAULT);
    let voltage_enabled = status.contains(StatusWord::VOLTAGE_ENABLED);
    let state_quick_stop = status.contains(StatusWord::QUICK_STOP);
    let state_switch_on_disabled = status.contains(StatusWord::SWITCH_ON_DISABLED);

    if state_fault {
        if state_operation_enabled && state_switched_on && state_ready_to_switch_on {
            return DriveState::FaultReactionActive;
        }
        return DriveState::Fault;
    }

    if !state_ready_to_switch_on
        && !state_switched_on
        && !state_operation_enabled
        && !state_switch_on_disabled
    {
        return DriveState::NotReadyToSwitchOn;
    }

    if state_switch_on_disabled {
        return DriveState::SwitchOnDisabled;
    }

    if !state_quick_stop {
        return DriveState::QuickStopActive;
    }

    if state_ready_to_switch_on && state_quick_stop && !state_switched_on {
        return DriveState::ReadyToSwitchOn;
    }

    if state_ready_to_switch_on && state_switched_on && voltage_enabled {
        if state_operation_enabled {
            return DriveState::OperationEnabled;
        }
        return DriveState::SwitchedOn;
    }

    DriveState::NotReadyToSwitchOn
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

        let mut foo = OutputPdo::unpack_from_slice(&output)?;

        let bar = InputPdo::unpack_from_slice(&input)?;
        let control_word =
            match parse_state(StatusWord::from_bits(bar.status_word).expect("Invalid status word"))
            {
                DriveState::SwitchOnDisabled => ControlWord::STATE_SHUTDOWN,
                DriveState::ReadyToSwitchOn | DriveState::SwitchedOn => {
                    ControlWord::STATE_ENABLE_OP
                }
                DriveState::OperationEnabled => ControlWord::STATE_ENABLE_OP,
                DriveState::Fault => ControlWord::STATE_FAULT_RESET,
                _ => ControlWord::STATE_DISABLE_VOLTAGE,
            };
        foo.control_word = control_word.bits();
        foo.speed = 1490;

        // let speed = uom::si::i16::AngularVelocity::new::<
        //     uom::si::angular_velocity::revolution_per_minute,
        // >(1000);

        // warn!("foo: {:?}", foo);

        foo.pack_to_slice(&mut *output)?;

        let bar = InputPdo::unpack_from_slice(&input)?;

        self.cnt += 1;

        if self.cnt % 1000 == 0 {
            let status_word = ethercrab::ds402::StatusWord::from_bits(bar.status_word);
            warn!("foo: {:?}", foo);
            warn!("bar: {:?}", bar);
            warn!("status_word: {:?}", status_word);
            warn!("control_word: {:?}", control_word.bits());
            warn!("output: {:?}", output);
            warn!(
                "state: {:?}",
                parse_state(status_word.expect("Invalid status word"))
            );
        }

        Ok(())
    }
}

impl DeviceInfo for I550 {
    const VENDOR_ID: u32 = 0x0000003b;
    const PRODUCT_ID: u32 = 0x69055000;
    const NAME: &'static str = "i550";
}
