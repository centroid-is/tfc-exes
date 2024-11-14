use crate::devices::device_trait::{Device, DeviceInfo};
use async_trait::async_trait;
use atomic_refcell::AtomicRefMut;
use ethercrab::{SubDevice, SubDevicePdi, SubDeviceRef};
use log::warn;
use std::error::Error;

pub struct I550;

static RX_PDO_ASSIGN: u16 = 0x1C12;
static TX_PDO_ASSIGN: u16 = 0x1C13;
static RX_PDO_MAPPING: u16 = 0x1605;
static TX_PDO_MAPPING: u16 = 0x1A05;
static BASIC_MOTOR_CONTROL: u16 = 0x2631;

#[async_trait]
impl Device for I550 {
    async fn setup<'maindevice, 'group>(
        &mut self,
        device: &mut SubDeviceRef<'maindevice, AtomicRefMut<'group, SubDevice>>,
    ) -> Result<(), Box<dyn Error>> {
        warn!("Setting up I550");

        // reset fault
        // device.sdo_write(BASIC_MOTOR_CONTROL, 4, 1 as u8).await?;

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

        device.sdo_write(BASIC_MOTOR_CONTROL, 0x01, 1 as u8).await?; // Set enable inverter to true

        // device
        //     .sdo_write(BASIC_MOTOR_CONTROL, 0x02, 1 as u8)
        //     .await?; // Set allow run to constant true

        warn!("I550 setup complete");
        Ok(())
    }
    async fn process_data<'maindevice, 'group>(
        &mut self,
        device: &mut SubDeviceRef<'maindevice, SubDevicePdi<'group>>,
    ) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}

impl DeviceInfo for I550 {
    const VENDOR_ID: u32 = 0x0000003b;
    const PRODUCT_ID: u32 = 0x69055000;
    const NAME: &'static str = "i550";
}
