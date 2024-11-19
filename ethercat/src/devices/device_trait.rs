use async_trait::async_trait;
use atomic_refcell::AtomicRefMut;
use ethercrab::{SubDevice, SubDevicePdi, SubDeviceRef};
use std::error::Error;

#[async_trait]
pub trait Device {
    async fn setup<'maindevice, 'group>(
        &mut self,
        device: &mut SubDeviceRef<'maindevice, AtomicRefMut<'group, SubDevice>>,
    ) -> Result<(), Box<dyn Error>>;
    async fn process_data<'maindevice, 'group>(
        &mut self,
        device: &mut SubDeviceRef<'maindevice, SubDevicePdi<'group>>,
    ) -> Result<(), Box<dyn Error>>;
}

pub trait DeviceInfo {
    const VENDOR_ID: u32;
    const PRODUCT_ID: u32;
    const NAME: &'static str;
}

pub struct UnimplementedDevice;

#[async_trait]
impl Device for UnimplementedDevice {
    async fn setup<'maindevice, 'group>(
        &mut self,
        _device: &mut SubDeviceRef<'maindevice, AtomicRefMut<'group, SubDevice>>,
    ) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    async fn process_data<'maindevice, 'group>(
        &mut self,
        device: &mut SubDeviceRef<'maindevice, SubDevicePdi<'group>>,
    ) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}

pub trait Index {
    const INDEX: u16;
    const SUBINDEX: u8;
}

pub trait WriteValueIndex {
    async fn sdo_write_value_index<T>(&mut self, value: T) -> Result<(), ethercrab::error::Error>
    where
        T: Index + ethercrab_wire::EtherCrabWireWrite;
}

impl<SubDeviceType> WriteValueIndex for SubDeviceRef<'_, SubDeviceType>
where
    SubDeviceType: std::ops::Deref<Target = SubDevice>,
{
    async fn sdo_write_value_index<T>(&mut self, value: T) -> Result<(), ethercrab::error::Error>
    where
        T: Index + ethercrab_wire::EtherCrabWireWrite,
    {
        self.sdo_write(T::INDEX, T::SUBINDEX, value).await
    }
}
