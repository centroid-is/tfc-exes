use arrayvec::ArrayVec;
use ethercrab::{
    std::{ethercat_now, tx_rx_task},
    MainDevice, MainDeviceConfig, PduStorage, SubDeviceGroup, Timeouts,
};
use log::{debug, error, info, trace, warn};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::time::Instant;
use std::{sync::Arc, time::Duration};
use tfc::confman::ConfMan;
use tfc::logger;
use tfc::progbase;
use tfc::time::MicroDuration;
use zbus;

mod devices;
use devices::device::make_device;
use devices::device_trait::Device;

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = 1100;
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Maximum total PDI length. // LENZE i550 requires 66 bytes
const PDI_LEN: usize = 66;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

#[derive(Deserialize, Serialize, JsonSchema, Default)]
struct BusConfig {
    pub interface: String,
    pub cycle_time: MicroDuration,
}

struct Bus {
    main_device: Arc<MainDevice<'static>>,
    config: ConfMan<BusConfig>,
    devices: ArrayVec<Box<dyn Device>, MAX_SUBDEVICES>,
    group: Option<SubDeviceGroup<MAX_SUBDEVICES, PDI_LEN, ethercrab::subdevice_group::Op>>,
    log_key: String,
}

impl Bus {
    pub fn new(conn: zbus::Connection) -> Self {
        let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
        let main_device = Arc::new(MainDevice::new(
            pdu_loop,
            Timeouts {
                state_transition: Duration::from_millis(10000),
                wait_loop_delay: Duration::from_millis(2),
                mailbox_response: Duration::from_millis(1000),
                ..Default::default()
            },
            MainDeviceConfig::default(),
        ));

        let config = ConfMan::<BusConfig>::new(conn.clone(), "bus").with_default(BusConfig {
            interface: "eth0".to_string(),
            cycle_time: Duration::from_millis(1).into(),
        });
        tokio::spawn(tx_rx_task(&config.read().interface, tx, rx).expect("spawn TX/RX task"));
        Self {
            main_device,
            config,
            devices: ArrayVec::new(),
            group: None,
            log_key: "ethercat".to_string(),
        }
    }
    pub async fn init(&mut self, dbus: zbus::Connection) -> Result<(), Box<dyn Error>> {
        self.devices.clear();
        let mut group = self
            .main_device
            .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
            .await?;
        let mut index: u16 = 0;
        for mut subdevice in group.iter(&self.main_device) {
            let identity = subdevice.identity();
            let mut device = make_device(
                dbus.clone(),
                identity.vendor_id,
                identity.product_id,
                index,
                subdevice.alias_address(),
            );
            // TODO: Make futures that can be awaited in parallel
            device.setup(&mut subdevice).await.map_err(|e| {
                warn!(target: &self.log_key, "Failed to setup device {}: {}", index, e);
                e
            })?;
            self.devices.push(device);
            index += 1;
        }
        trace!(target: &self.log_key, "Setup complete for devices: {}", index);

        let group = group.into_op(&self.main_device).await?;
        self.group = Some(group);

        trace!(target: &self.log_key, "Now in operational state");

        Ok(())
    }
    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        let ref mut group = self.group.as_mut().expect("Group not initialized");

        let mut tick_interval = tokio::time::interval(self.config.read().cycle_time.into());
        info!(target: &self.log_key, "Ethercat tick interval: {:?}", self.config.read().cycle_time);
        tick_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut cnt = 0;
        let mut instant = Instant::now();
        let mut tx_rx_duration = Duration::ZERO;
        let mut process_data_duration = Duration::ZERO;
        loop {
            let mut tx_rx_instant = Instant::now();
            group.tx_rx(&self.main_device).await?;
            tx_rx_duration += tx_rx_instant.elapsed();

            let mut process_data_instant = Instant::now();
            for (device_index, mut subdevice) in group.iter(&self.main_device).enumerate() {
                if let Some(device) = self.devices.get_mut(device_index) {
                    device.process_data(&mut subdevice).await?;
                }
            }
            process_data_duration += process_data_instant.elapsed();

            tick_interval.tick().await;
            cnt += 1;
            if cnt % 1000 == 0 {
                info!(target: &self.log_key, "Ethercat tick interval: {:?}", instant.elapsed()/1000);
                info!(target: &self.log_key, "Tx/Rx duration: {:?}", tx_rx_duration/1000);
                info!(target: &self.log_key, "Process data duration: {:?}", process_data_duration/1000);
                instant = Instant::now();
                tx_rx_duration = Duration::ZERO;
                process_data_duration = Duration::ZERO;
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    progbase::init();
    logger::init_combined_logger()?;
    trace!(target: "ethercat", "Starting ethercat");
    let formatted_name = format!(
        "is.centroid.{}.{}",
        progbase::exe_name(),
        progbase::proc_name()
    );
    let dbus = zbus::connection::Builder::system()?
        .name(formatted_name)?
        .build()
        .await?;

    let mut bus = Bus::new(dbus.clone());

    loop {
        loop {
            let res = bus.init(dbus.clone()).await;
            if let Err(e) = res {
                warn!(target: &bus.log_key, "Failed to init: {}", e);
            } else {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }

        let _ = bus.run().await.map_err(|e| {
            error!(target: &bus.log_key, "Failed to run will retry: {}", e);
            e
        });
    }

    // let _ = Arc::new(Mutex::new(OperationsImpl::new(bus.clone())));

    std::future::pending::<()>().await;
    Ok(())
}
