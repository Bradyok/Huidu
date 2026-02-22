//! Connection backends for HDSet.
//!
//! HDSet can communicate with Huidu hardware via two paths:
//!
//! 1. [`tcp`] — TCP connection to a BoxPlayer (embedded Linux device) on port 10001.
//!    This is the primary path for network-connected displays.
//!
//! 2. [`serial`] — Direct USB/serial connection to a Huidu send card (VP/T/A/B/C/D-series).
//!    Used for hardware that is directly USB-connected to the PC, bypassing BoxPlayer.
//!    Send cards use a CH341 or CH343 USB-to-serial adapter, or USB bulk mode (H-series).
//!
//! The serial protocol to send cards is a different, lower-level binary protocol
//! than the TCP SDK protocol.  It requires further reverse engineering to implement
//! fully.  This module provides the infrastructure and a stub.

pub mod serial;

// tcp module is not needed here — HdsetClient in client.rs already handles TCP.
