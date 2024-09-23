use derive_more::Display;
use log::{info, trace};
use parking_lot::ReentrantMutex;
use serde::{Deserialize, Serialize};
use smlang::statemachine;
use std::cell::RefCell;
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::{Arc, Mutex, Weak};
use tfc::ipc::{Base, Signal, Slot};
use tfc::logger;
use tfc::progbase;
use tokio;
use zbus::interface;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, zbus::zvariant::Type)]
pub enum OperationMode {
    Unknown = 0,
    Stopped = 1,
    Starting = 2,
    Running = 3,
    Stopping = 4,
    Cleaning = 5,
    Emergency = 6,
    Fault = 7,
    Maintenance = 8,
}

struct OperationsClient {
    log_key: String,
}

impl OperationsClient {
    pub fn new(key: &str) -> Self {
        Self {
            log_key: key.to_string(),
        }
    }
}
#[interface(name = "is.centroid.OperationMode")]
impl OperationsClient {
    #[zbus(property)]
    async fn mode(&self) -> Result<String, zbus::fdo::Error> {
        Ok("manual".to_string())
    }

    #[zbus(signal)]
    async fn update(
        signal_ctxt: &zbus::SignalContext<'_>,
        new_mode: &str,
        old_mode: &str,
    ) -> zbus::Result<()> {
        println!(
            "Operation mode updated from {:?} to {:?}",
            old_mode, new_mode
        );
        Ok(())
    }

    fn set_mode(&self, mode: &str) -> Result<(), zbus::fdo::Error> {
        println!("Setting operation mode to {:?}", mode);
        Ok(())
    }

    fn stop_with_reason(&self, reason: &str) -> Result<(), zbus::fdo::Error> {
        println!("Stopping with reason: {}", reason);
        Ok(())
    }
}

// Define the state machine transitions and actions
statemachine! {
    name: Operations,
    derive_states: [Debug, Display],
    derive_events: [Debug, Display],
    transitions: {
        *Init + SetStopped / transition_to_stopped = Stopped,

        Stopped + SetStarting / transition_to_starting = Starting,
        Stopped + RunButton / transition_to_starting = Starting,
        Starting + StartingTimeout / transition_to_running = Running,
        Starting + StartingFinished / transition_to_running = Running,
        Running + RunButton / transition_to_stopping = Stopping,
        Running + SetStopped / transition_to_stopping = Stopping,
        Stopping + StoppingTimeout / transition_to_stopped = Stopped,
        Stopping + StoppingFinished / transition_to_stopped = Stopped,

        Stopped + CleaningButton / transition_to_cleaning = Cleaning,
        Stopped + SetCleaning / transition_to_cleaning = Cleaning,
        Cleaning + CleaningButton / transition_to_stopped = Stopped,
        Cleaning + SetStopped / transition_to_stopped = Stopped,

        // Transitions to Emergency
        _ + SetEmergency / transition_to_emergency = Emergency,

        _ + EmergencyOn / transition_to_emergency = Emergency,

        Emergency + EmergencyOff [!is_fault] / transition_to_stopped = Stopped,
        Emergency + EmergencyOff [is_fault] / transition_to_fault = Fault,

        Stopped + FaultOn / transition_to_fault = Fault,
        Stopped + SetFault / transition_to_fault = Fault,
        Running + FaultOn / transition_to_fault = Fault,
        Running + SetFault / transition_to_fault = Fault,

        Fault + FaultOff / transition_to_stopped = Stopped,

        Stopped + MaintenanceButton / transition_to_maintenance = Maintenance,
        Stopped + SetMaintenance / transition_to_maintenance = Maintenance,
        Maintenance + MaintenanceButton / transition_to_stopped = Stopped,
        Maintenance + SetStopped / transition_to_stopped = Stopped,
    },
}

pub struct Context {
    owner: *mut OperationsImpl,
    log_key: String,
}

pub struct OperationsImpl {
    log_key: String,
    stopped: Signal<bool>,
    starting: Signal<bool>,
    running: Signal<bool>,
    stopping: Signal<bool>,
    cleaning: Signal<bool>,
    emergency_out: Signal<bool>,
    fault_out: Signal<bool>,
    maintenance_out: Signal<bool>,
    mode: Signal<u64>,
    mode_str: Signal<String>,
    starting_finished: Slot<bool>,
    stopping_finished: Slot<bool>,
    run_button: Slot<bool>,
    cleaning_button: Slot<bool>,
    maintenance_button: Slot<bool>,
    emergency_in: Slot<bool>,
    fault_in: Slot<bool>,
    sm: Option<OperationsStateMachine<Context>>,
}

impl OperationsImpl {
    pub fn new(bus: zbus::Connection) -> Arc<Mutex<Self>> {
        let bus_cp = bus.clone();
        let path = "/is/centroid/OperationMode";
        let client = OperationsClient::new("OperationMode");
        let log_key = "OperationMode";
        tokio::spawn(async move {
            // log if error
            let _ = bus_cp
                .object_server()
                .at(path, client)
                .await
                .expect(&format!("Error registering object: {}", log_key));
        });
        let ctx = Arc::new(Mutex::new(Self {
            log_key: log_key.to_string(),
            stopped: Signal::new(
                bus.clone(),
                Base::new("stopped", Some("System state stopped")),
            ),
            starting: Signal::new(
                bus.clone(),
                Base::new("starting", Some("System state starting")),
            ),
            running: Signal::new(
                bus.clone(),
                Base::new("running", Some("System state running")),
            ),
            stopping: Signal::new(
                bus.clone(),
                Base::new("stopping", Some("System state stopping")),
            ),
            cleaning: Signal::new(
                bus.clone(),
                Base::new("cleaning", Some("System state cleaning")),
            ),
            emergency_out: Signal::new(
                bus.clone(),
                Base::new("emergency", Some("System state emergency")),
            ),
            fault_out: Signal::new(bus.clone(), Base::new("fault", Some("System state fault"))),
            maintenance_out: Signal::new(
                bus.clone(),
                Base::new("maintenance", Some("System state maintenance")),
            ),
            mode: Signal::new(bus.clone(), Base::new("mode", Some("Current system mode"))),
            mode_str: Signal::new(
                bus.clone(),
                Base::new("mode_str", Some("Current system mode as string")),
            ),
            starting_finished: Slot::new(
                bus.clone(),
                Base::new(
                    "starting_finished",
                    Some("Starting finished, when certain initalization steps are done"),
                ),
            ),
            stopping_finished: Slot::new(
                bus.clone(),
                Base::new(
                    "stopping_finished",
                    Some("Stopping finished, when certain shutdown sequence is done"),
                ),
            ),
            run_button: Slot::new(
                bus.clone(),
                Base::new("run_button", Some("Run button pressed")),
            ),
            cleaning_button: Slot::new(
                bus.clone(),
                Base::new("cleaning_button", Some("Cleaning button pressed")),
            ),
            maintenance_button: Slot::new(
                bus.clone(),
                Base::new("maintenance_button", Some("Maintenance button pressed")),
            ),
            emergency_in: Slot::new(
                bus.clone(),
                Base::new(
                    "emergency",
                    Some("Emergency input signal, true when emergency is active"),
                ),
            ),
            fault_in: Slot::new(
                bus.clone(),
                Base::new(
                    "fault",
                    Some("Fault input signal, true when fault is active"),
                ),
            ),
            sm: None,
        }));

        let mut ops_impl = ctx.lock().unwrap();
        let ops_impl_ptr: *mut OperationsImpl = &mut *ops_impl as *mut OperationsImpl;
        drop(ops_impl);
        let context = Context {
            log_key: "StateMachine".to_string(),
            owner: ops_impl_ptr,
        };

        ctx.lock().unwrap().sm = Some(OperationsStateMachine::new(context));

        let shared_ctx = Arc::clone(&ctx);
        ctx.lock()
            .unwrap()
            .starting_finished
            .recv(Box::new(move |val: &bool| {
                shared_ctx.lock().unwrap().on_starting_finished(val);
            }));
        let shared_ctx = Arc::clone(&ctx);
        ctx.lock()
            .unwrap()
            .stopping_finished
            .recv(Box::new(move |val: &bool| {
                shared_ctx.lock().unwrap().on_stopping_finished(val);
            }));
        let shared_ctx = Arc::clone(&ctx);
        ctx.lock()
            .unwrap()
            .run_button
            .recv(Box::new(move |val: &bool| {
                shared_ctx.lock().unwrap().on_run_button(val);
            }));
        let shared_ctx = Arc::clone(&ctx);
        ctx.lock()
            .unwrap()
            .cleaning_button
            .recv(Box::new(move |val: &bool| {
                shared_ctx.lock().unwrap().on_cleaning_button(val);
            }));
        let shared_ctx = Arc::clone(&ctx);
        ctx.lock()
            .unwrap()
            .maintenance_button
            .recv(Box::new(move |val: &bool| {
                shared_ctx.lock().unwrap().on_maintenance_button(val);
            }));
        let shared_ctx = Arc::clone(&ctx);
        ctx.lock()
            .unwrap()
            .emergency_in
            .recv(Box::new(move |val: &bool| {
                shared_ctx.lock().unwrap().on_emergency_in(val);
            }));
        let shared_ctx = Arc::clone(&ctx);
        ctx.lock()
            .unwrap()
            .fault_in
            .recv(Box::new(move |val: &bool| {
                shared_ctx.lock().unwrap().on_fault_in(val);
            }));

        ctx
    }

    // fn set_sm(&mut self, sm: OperationsStateMachine<Arc<ReentrantMutex<RefCell<Self>>>>) {
    //     self.sm = Some(sm);
    // }

    fn on_starting_finished(&mut self, val: &bool) {
        trace!(target: &self.log_key, "Starting finished: {}", val);
        if !val {
            return;
        }

        let _ = self
            .sm
            .as_mut()
            .unwrap()
            .process_event(OperationsEvents::StartingFinished)
            .map_err(|e| info!(target: &self.log_key,"Error processing event: {:?}", e));
    }
    fn on_stopping_finished(&mut self, val: &bool) {
        trace!(target: &self.log_key, "Stopping finished: {}", val);
        if !val {
            return;
        }

        let _ = self
            .sm
            .as_mut()
            .unwrap()
            .process_event(OperationsEvents::StoppingFinished)
            .map_err(|e| info!(target: &self.log_key,"Error processing event: {:?}", e));
    }
    fn on_run_button(&mut self, val: &bool) {
        trace!(target: &self.log_key, "Run button pressed: {}", val);
        if !val {
            return;
        }

        let _ = self
            .sm
            .as_mut()
            .unwrap()
            .process_event(OperationsEvents::RunButton)
            .map_err(|e| info!(target: &self.log_key,"Error processing event: {:?}", e));
    }
    fn on_cleaning_button(&mut self, val: &bool) {
        trace!(target: &self.log_key, "Cleaning button pressed: {}", val);
        if !val {
            return;
        }

        let _ = self
            .sm
            .as_mut()
            .unwrap()
            .process_event(OperationsEvents::CleaningButton)
            .map_err(|e| info!(target: &self.log_key,"Error processing event: {:?}", e));
    }
    fn on_maintenance_button(&mut self, val: &bool) {
        trace!(target: &self.log_key, "Maintenance button pressed: {}", val);
        if !val {
            return;
        }

        let _ = self
            .sm
            .as_mut()
            .unwrap()
            .process_event(OperationsEvents::MaintenanceButton)
            .map_err(|e| info!(target: &self.log_key,"Error processing event: {:?}", e));
    }
    fn on_emergency_in(&mut self, val: &bool) {
        trace!(target: &self.log_key, "Emergency input signal: {}", val);
        if *val {
            println!("Emergency on");
            let _ = self
                .sm
                .as_mut()
                .unwrap()
                .process_event(OperationsEvents::EmergencyOn)
                .map_err(|e| info!(target: &self.log_key,"Error processing event: {:?}", e));
            println!("Emergency on done");
        } else {
            println!("Emergency off");
            let _ = self
                .sm
                .as_mut()
                .unwrap()
                .process_event(OperationsEvents::EmergencyOff)
                .map_err(|e| info!(target: &self.log_key,"Error processing event: {:?}", e));
            println!("Emergency off done");
        }
    }
    fn on_fault_in(&mut self, val: &bool) {
        trace!(target: &self.log_key, "Fault input signal: {}", val);
        // if *val {
        //     let _ = self
        //         .sm
        //         .as_mut()
        //         .unwrap()
        //         .process_event(OperationsEvents::FaultOn)
        //         .map_err(|e| info!(target: &self.log_key,"Error processing event: {:?}", e));
        // } else {
        //     let _ = self
        //         .sm
        //         .as_mut()
        //         .unwrap()
        //         .process_event(OperationsEvents::FaultOff)
        //         .map_err(|e| info!(target: &self.log_key,"Error processing event: {:?}", e));
        // }
    }
    fn on_entry_stopped(&mut self) {
        println!("Entering Stopped state");
    }
    fn on_exit_stopped(&mut self) {
        println!("Exiting Stopped state");
    }
    fn on_entry_starting(&mut self) {
        println!("Entering Starting state");
    }
    fn on_exit_starting(&mut self) {
        println!("Exiting Starting state");
    }
    fn on_entry_running(&mut self) {
        println!("Entering Running state");
    }
    fn on_exit_running(&mut self) {
        println!("Exiting Running state");
    }
    fn on_entry_stopping(&mut self) {
        println!("Entering Stopping state");
    }
    fn on_exit_stopping(&mut self) {
        println!("Exiting Stopping state");
    }
    fn on_entry_cleaning(&mut self) {
        println!("Entering Cleaning state");
    }
    fn on_exit_cleaning(&mut self) {
        println!("Exiting Cleaning state");
    }
    fn on_entry_emergency(&mut self) {
        println!("Entering Emergency state");
    }
    fn on_exit_emergency(&mut self) {
        println!("Exiting Emergency state");
    }
    fn on_entry_fault(&mut self) {
        println!("Entering Fault state");
    }
    fn on_exit_fault(&mut self) {
        println!("Exiting Fault state");
    }
    fn on_entry_maintenance(&mut self) {
        println!("Entering Maintenance state");
    }
    fn on_exit_maintenance(&mut self) {
        println!("Exiting Maintenance state");
    }

    fn is_fault(&self) -> Result<bool, ()> {
        Ok(false)
    }
}

// todo: make this safe
unsafe impl Send for Context {}
unsafe impl Sync for Context {}

impl Context {
    fn with_owner<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut OperationsImpl) -> R,
    {
        // this should be safe, this is just circular dependency hell
        // todo: make this safe
        unsafe {
            let ops_impl = &mut *self.owner;
            f(ops_impl)
        }
    }
}

impl OperationsStateMachineContext for Context {
    fn on_entry_stopped(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_entry_stopped();
        });
    }
    fn on_exit_stopped(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_exit_stopped();
        });
    }
    fn on_entry_starting(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_entry_starting();
        });
    }
    fn on_exit_starting(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_exit_starting();
        });
    }
    fn on_entry_running(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_entry_running();
        });
    }
    fn on_exit_running(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_exit_running();
        });
    }
    fn on_entry_stopping(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_entry_stopping();
        });
    }
    fn on_exit_stopping(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_exit_stopping();
        });
    }
    fn on_entry_cleaning(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_entry_cleaning();
        });
    }
    fn on_exit_cleaning(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_exit_cleaning();
        });
    }
    fn on_entry_emergency(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_entry_emergency();
        });
    }
    fn on_exit_emergency(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_exit_emergency();
        });
    }
    fn on_entry_fault(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_entry_fault();
        });
    }
    fn on_exit_fault(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_exit_fault();
        });
    }
    fn on_entry_maintenance(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_entry_maintenance();
        });
    }
    fn on_exit_maintenance(&mut self) {
        self.with_owner(|ops_impl| {
            ops_impl.on_exit_maintenance();
        });
    }
    fn is_fault(&self) -> Result<bool, ()> {
        self.with_owner(|ops_impl| ops_impl.is_fault())
    }
    // Transition actions
    fn transition_to_stopped(&mut self) -> Result<(), ()> {
        let to_state = OperationsStates::Stopped;
        Ok(())
    }
    fn transition_to_starting(&mut self) -> Result<(), ()> {
        let to_state = OperationsStates::Starting;
        Ok(())
    }
    fn transition_to_running(&mut self) -> Result<(), ()> {
        let to_state = OperationsStates::Running;
        Ok(())
    }
    fn transition_to_stopping(&mut self) -> Result<(), ()> {
        let to_state = OperationsStates::Stopping;
        Ok(())
    }
    fn transition_to_cleaning(&mut self) -> Result<(), ()> {
        let to_state = OperationsStates::Cleaning;
        Ok(())
    }

    fn transition_to_emergency(&mut self) -> Result<(), ()> {
        let to_state = OperationsStates::Emergency;
        Ok(())
    }
    fn transition_to_fault(&mut self) -> Result<(), ()> {
        let to_state = OperationsStates::Fault;
        Ok(())
    }
    fn transition_to_maintenance(&mut self) -> Result<(), ()> {
        let to_state = OperationsStates::Maintenance;
        Ok(())
    }

    fn log_process_event(&self, current_state: &OperationsStates, event: &OperationsEvents) {
        trace!(target: &self.log_key,
            "[StateMachineLogger]\t[{:?}] Processing event {:?}",
            current_state, event
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    progbase::init();
    logger::init_combined_logger()?;
    let formatted_name = format!(
        "is.centroid.{}.{}",
        progbase::exe_name(),
        progbase::proc_name()
    );
    let bus = zbus::connection::Builder::system()?
        .name(formatted_name)?
        .build()
        .await?;

    let _ = Arc::new(Mutex::new(OperationsImpl::new(bus.clone())));

    std::future::pending::<()>().await;
    Ok(())
}
