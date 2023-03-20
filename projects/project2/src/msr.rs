//! MSR handlers to test msr instructions correctly implemented.
use alloc::boxed::Box;
use crate::vmexit::msr::Msr;
use alloc::collections::LinkedList;
use keos::spin_lock::SpinLock;
use kev::vcpu::GenericVCpuState;
use kev::{Probe, VmError};

/// emulation of a msr that mimics the behavior of stack.
#[derive(Default)]
pub struct StackMsr {
    stack: SpinLock<LinkedList<u64>>,
}
impl StackMsr {
    /// Create a new StackMsr.
    pub(crate) fn new() -> Self {
        Self {
            stack: SpinLock::new(LinkedList::new()),
        }
    }
}
impl Msr for StackMsr {
    fn rdmsr(
        &self,
        _index: u32,
        _p: &dyn Probe,
        _generic_vcpu_state: &mut GenericVCpuState,
    ) -> Result<u64, VmError> {
        self.stack
            .lock()
            .pop_front()
            .ok_or(VmError::ControllerError(Box::new("Empty stack")))
    }

    fn wrmsr(
        &mut self,
        _index: u32,
        value: u64,
        _p: &dyn Probe,
        _generic_vcpu_state: &mut GenericVCpuState,
    ) -> Result<(), VmError> {
        self.stack.lock().push_back(value);
        Ok(())
    }
}
