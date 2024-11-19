use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWrite};
// bitflags! {
//     #[derive(Debug, EtherCrabWireReadWrite)]
//     pub struct StatusWord: u16 {
//         /// toggeles 0->1->0 with each updated data set
//         const TxPDO = (1 << 15);
//         const Unused14 = (1 << 14);
//         /// Synchronization error
//         const SyncError = (1 << 13);
//         const Unused12 = (1 << 12);
//         const Unused11 = (1 << 11);
//         const Unused10 = (1 << 10);
//         const Unused9 = (1 << 9);
//         /// Steady state (Idling recognition)
//         /// If the load value remains within a range of values y for longer than time x, then the SteadyState is
//         /// activated in the StatusWord.
//         /// TODO: The parameters x and y can be specified in the CoE
//         const SteadyState = (1 << 8);
//         const CalibrationInProgress = (1 << 7);
//         const CollectiveError = (1 << 6);
//         const Unused5 = (1 << 5);
//         const Unused4 = (1 << 4);
//         const DataInvalid = (1 << 3);
//         const Unused2 = (1 << 2);
//         const OverRange = (1 << 1);
//         const Unused0 = (1 << 0);
//     }
//     #[derive(Debug, EtherCrabWireReadWrite)]
//     pub struct ControlWord: u16 {
//         const Unused15 = (1 << 15);
//         const Unused14 = (1 << 14);
//         const Unused13 = (1 << 13);
//         const Unused12 = (1 << 12);
//         const Unused11 = (1 << 11);
//         const Unused10 = (1 << 10);
//         const Unused9 = (1 << 9);
//         const Unused8 = (1 << 8);
//         const Unused7 = (1 << 7);
//         const Unused6 = (1 << 6);
//         const Unused5 = (1 << 5);
//         /// When taring, the scales are set to zero using an arbitrary applied load; i.e. an offset correction is performed.
//         /// The EL3356 supports two tarings; it is recommended to set a strong filter when taring.
//         /// Temporary tare: The correction value is NOT stored in the terminal and is lost in the event of a power failure.
//         /// Permanent tare: The correction value is stored locally in the terminal's EEPROM and is not lost in the event of a power
//         /// failure.
//         const Tare = (1 << 4);
//         /// Mode 0: High precision Analog conversion at 10.5 kSps (samples per second) Slow conversion and thus high accuracy
//         /// Typical Latency 7.2 ms
//         /// Mode 1: High speed / low latency Analog conversion at 105.5 kSps (samples per second) Fast conversion with low latency
//         /// Typical Latency 0.72 ms
//         const SampleMode = (1 << 3);
//         /// If the terminal is placed in the freeze state by InputFreeze in the control word, no further analog measured
//         /// values are relayed to the internal filter. This function is usable, for example, if a filling surge is expected from
//         /// the application that would unnecessarily overdrive the filters due to the force load. This would result in a
//         /// certain amount of time elapsing until the filter had settled again. The user himself must determine a sensible
//         /// InputFreeze time for his filling procedure.
//         const InputFreeze = (1 << 2);
//         /// The measuring amplifiers are periodically subjected to examination and self-calibration. Several analog
//         /// switches are provided for this purpose, so that the various calibration signals can be connected. It is
//         /// important for this process that the entire signal path, including all passive components, is examined at every
//         /// phase of the calibration. Only the interference suppression elements (L/C combination) and the analog
//         /// switches themselves cannot be examined. In addition, a self-test is carried out at longer intervals.
//         /// The self-calibration is carried out every three minutes in the default setting.
//         /// Self-calibration
//         /// The time interval is set in 100 ms steps with object 0x8000:31 [} 174]; default: 3 min.
//         /// Duration approx. 150 ms
//         /// Self-test
//         /// is additional carried out together with every nth self-calibration.
//         /// The multiple (n) is set with object 0x8000:32 [} 174]; default: 10
//         /// additional duration approx. 70 ms.
//         const DisableCalibration = (1 << 1);
//         /// Starts the self-calibration immediately
//         const StartCalibration = (1 << 0);
//     }
// }

// pub struct TxPdo {
//     pub control_word: ControlWord,
// }
