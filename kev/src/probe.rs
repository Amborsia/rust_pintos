use crate::{
    vm::{Gpa, Gva},
    vmcs::ActiveVmcs,
};
use abyss::addressing::{Pa, Va};

/// Traits to probe vcpu internal state.
pub trait Probe {
    /// Translate guest physical address to host physical address
    fn gpa2hpa(&self, vmcs: &ActiveVmcs, gpa: Gpa) -> Option<Pa>;
    /// Translate guest virtual address to host physical address
    fn gva2hpa(&self, vmcs: &ActiveVmcs, gva: Gva) -> Option<Pa>;
    /// Translate guest physical address to host virtual address
    #[inline]
    fn gpa2hva(&self, vmcs: &ActiveVmcs, gpa: Gpa) -> Option<Va> {
        self.gpa2hpa(vmcs, gpa).map(|pa| pa.into_va())
    }
    /// Translate guest virtual address to host virtual address
    #[inline]
    fn gva2hva(&self, vmcs: &ActiveVmcs, gva: Gva) -> Option<Va> {
        self.gva2hpa(vmcs, gva).map(|pa| pa.into_va())
    }
}
