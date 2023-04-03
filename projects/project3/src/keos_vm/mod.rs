//! Vm to run keos.

use crate::{keos_vm::dev::PciPio, vmexit::mmio};
use alloc::sync::Arc;
use keos::{fs::file_system, mm::Page, spin_lock::SpinLock};
use kev::{
    vcpu::{Cr0, Cr4, GenericVCpuState, Rflags, VmexitResult},
    vm_control::*,
    vmcs::{ActiveVmcs, Field},
    vmexits::VmexitController,
    VmError,
};
use pager::KernelVmPager;
use project2::{
    hypercall::HypercallCtx,
    vmexit::{cpuid, hypercall, msr, pio},
};

pub mod dev;
pub mod elf;
pub mod pager;

/// The Vmstate of VmBase.
pub struct VmState {
    pager: Arc<SpinLock<KernelVmPager>>,
    io_bmap: Arc<(Page, Page)>,
}

impl VmState {
    pub fn new(ram_in_kib: usize) -> Option<Self> {
        let (mut io_bmap_a, mut io_bmap_b) = (Page::new()?, Page::new()?);
        unsafe {
            io_bmap_a.inner_mut().fill(0xff);
            io_bmap_b.inner_mut().fill(0xff);
            for port in [
                0x3f8, 0x3f9, 0x3fa, 0x3fb, 0x3fc, 0x3fd, 0x84, // Serial series.
                0x20, 0x21, 0xa0, 0xa1, // 8259A interrupt controller series.
                0x42, 0x43, 0x61, // PIT
            ] {
                if port >= 0x8000 {
                    let p = port - 0x8000;
                    io_bmap_b.inner_mut()[p / 8] &= !(1 << (p % 8));
                } else {
                    let p = port;
                    io_bmap_a.inner_mut()[p / 8] &= !(1 << (p % 8));
                }
            }
        }

        let io_bmap = Arc::new((io_bmap_a, io_bmap_b));
        let pager = Arc::new(SpinLock::new(KernelVmPager::from_image(
            file_system()
                .expect("Filesystem is not exist.")
                .open("gKeOS")
                .expect("gKeOS is not exist."),
            ram_in_kib,
        )?));
        Some(VmState { pager, io_bmap })
    }
}

impl kev::vm::VmState for VmState {
    type VcpuState = VcpuState;
    type Error = VmError;

    fn vcpu_state(&self) -> Self::VcpuState {
        let (mmio_ctl, mut pio_ctl, hypercall_ctl, cpuid_ctl, mut msr_ctl) = (
            mmio::Controller::new(),
            pio::Controller::new(),
            hypercall::Controller::new(HypercallCtx),
            cpuid::Controller::new(),
            msr::Controller::new(),
        );

        assert!(msr_ctl.insert(0xC000_0080, dev::EferMsr::default()));
        assert!(msr_ctl.insert(0x4b56_4d01, dev::KvmSystemTimeNew::default()));
        assert!(msr_ctl.insert(0x12, dev::KvmSystemTimeNew::default()));
        dev::X2Apic::attach(&mut msr_ctl);
        assert!(pio_ctl.register(0xCF8, PciPio));
        assert!(pio_ctl.register(0xCFC, PciPio));

        VcpuState {
            pager: self.pager.clone(),
            vmexit_controller: (mmio_ctl, (pio_ctl, (hypercall_ctl, (cpuid_ctl, msr_ctl)))),
            io_bmap: self.io_bmap.clone(),
        }
    }

    fn setup_vbsp(
        &self,
        vbsp_generic_state: &mut GenericVCpuState,
        vbsp_vcpu_state: &mut Self::VcpuState,
    ) -> Result<(), Self::Error> {
        vbsp_generic_state
            .vmcs
            .write(Field::GuestRip, self.pager.lock().entry() as u64)?;
        vbsp_generic_state.vmcs.write(Field::GuestRsp, 0xa0000)?;
        vbsp_generic_state.gprs.rsi = vbsp_vcpu_state
            .pager
            .lock()
            .finalize_mem()
            .expect("Failed to finalize the memory.");

        let vmcs = &vbsp_generic_state.vmcs;
        vmcs.write(Field::GuestCsSelector, 0x10)?;
        vmcs.write(Field::GuestCsBase, 0)?;
        vmcs.write(Field::GuestCsLimit, 0xffffffff)?;
        vmcs.write(Field::GuestCsAccessRights, 0xc09b)?;

        vmcs.write(Field::GuestEsSelector, 0x18)?;
        vmcs.write(Field::GuestEsBase, 0)?;
        vmcs.write(Field::GuestEsLimit, 0xffffffff)?;
        vmcs.write(Field::GuestEsAccessRights, 0xc093)?;

        vmcs.write(Field::GuestSsSelector, 0x18)?;
        vmcs.write(Field::GuestSsBase, 0)?;
        vmcs.write(Field::GuestSsLimit, 0xffffffff)?;
        vmcs.write(Field::GuestSsAccessRights, 0xc093)?;

        vmcs.write(Field::GuestDsSelector, 0x18)?;
        vmcs.write(Field::GuestDsBase, 0)?;
        vmcs.write(Field::GuestDsLimit, 0xffffffff)?;
        vmcs.write(Field::GuestDsAccessRights, 0xc093)?;

        vmcs.write(Field::GuestFsSelector, 0x18)?;
        vmcs.write(Field::GuestFsBase, 0)?;
        vmcs.write(Field::GuestFsLimit, 0xffffffff)?;
        vmcs.write(Field::GuestFsAccessRights, 0xc093)?;

        vmcs.write(Field::GuestGsSelector, 0x18)?;
        vmcs.write(Field::GuestGsBase, 0)?;
        vmcs.write(Field::GuestGsLimit, 0xffffffff)?;
        vmcs.write(Field::GuestGsAccessRights, 0xc093)?;

        vmcs.write(Field::GuestTrSelector, 0)?;
        vmcs.write(Field::GuestTrBase, 0)?;
        vmcs.write(Field::GuestTrLimit, 0)?;
        vmcs.write(Field::GuestTrAccessRights, 0x8b)?;

        vmcs.write(Field::GuestLdtrSelector, 0)?;
        vmcs.write(Field::GuestLdtrBase, 0)?;
        vmcs.write(Field::GuestLdtrLimit, 0)?;
        vmcs.write(Field::GuestLdtrAccessRights, 0x82)?;

        vmcs.write(Field::GuestGdtrBase, 0)?;
        vmcs.write(Field::GuestGdtrLimit, 0)?;

        vmcs.write(Field::GuestIdtrBase, 0)?;
        vmcs.write(Field::GuestIdtrLimit, 0)?;
        // CR0=00000011 CR3=00000000 CR4=00000000
        vmcs.write(Field::GuestCr0, (Cr0::NE | Cr0::PE).bits())?;
        vmcs.write(Field::GuestCr3, 0)?;
        vmcs.write(Field::GuestCr4, Cr4::VMXE.bits())?;

        // Guest non-register state.
        vmcs.write(Field::GuestActivityState, 0)?;
        vmcs.write(Field::GuestInterruptibilityState, 0)?;
        vmcs.write(Field::GuestLinkPointer, 0xffff_ffff)?;
        vmcs.write(Field::GuestLinkPointerHi, 0xffff_ffff)?;

        // DR7=0000000000000400
        vmcs.write(Field::GuestDr7, 0)?;
        vmcs.write(Field::GuestIa32Debugctl, 0)?;

        // EFL=00200002
        vmcs.write(Field::GuestRflags, Rflags::_1.bits())?;

        Ok(())
    }
}

/// The Vcpu state of NoEptVmState.
pub struct VcpuState {
    pager: Arc<SpinLock<KernelVmPager>>,
    vmexit_controller: (
        mmio::Controller,
        (
            pio::Controller,
            (
                hypercall::Controller<HypercallCtx>,
                (cpuid::Controller, msr::Controller),
            ),
        ),
    ),
    io_bmap: Arc<(Page, Page)>,
}

impl kev::vcpu::VCpuState for VcpuState {
    fn pinbase_ctls(&self) -> VmcsPinBasedVmexecCtl {
        VmcsPinBasedVmexecCtl::EXTERNAL_INTERRUPT_EXITING
    }
    fn procbase_ctls(&self) -> VmcsProcBasedVmexecCtl {
        VmcsProcBasedVmexecCtl::HLT_EXITING
            | VmcsProcBasedVmexecCtl::UNCONDIOEXIT
            | VmcsProcBasedVmexecCtl::USEIOBMP
    }
    fn procbase_ctls2(&self) -> VmcsProcBasedSecondaryVmexecCtl {
        VmcsProcBasedSecondaryVmexecCtl::ENABLE_RDTSCP
            | VmcsProcBasedSecondaryVmexecCtl::ENABLE_EPT
            | VmcsProcBasedSecondaryVmexecCtl::UNRESTRICTED_GUEST
    }
    fn entry_ctls(&self) -> VmcsEntryCtl {
        VmcsEntryCtl::LOAD_IA32_EFER
    }
    fn exit_ctls(&self) -> VmcsExitCtl {
        VmcsExitCtl::ACK_INTR_ON_EXIT
            | VmcsExitCtl::HOST_ADDRESS_SPACE_SIZE
            | VmcsExitCtl::SAVE_IA32_EFER
            | VmcsExitCtl::LOAD_IA32_EFER
    }
    fn init_guest_state(&self, vmcs: &ActiveVmcs) -> Result<(), VmError> {
        vmcs.write(Field::Eptptr, unsafe {
            self.pager.lock().ept_ptr().into_usize() as u64 | (3 << 3) | 6
        })?;

        // If the “use I/O bitmaps” VM-execution control is 1, bits 11:0 of each I/O-bitmap address must be 0. Neither
        // address should set any bits beyond the processor’s physical-address width.1,2
        {
            vmcs.write(
                Field::IoBitmapA,
                unsafe { self.io_bmap.0.pa().into_usize() } as u64,
            )?;
            vmcs.write(
                Field::IoBitmapB,
                unsafe { self.io_bmap.1.pa().into_usize() } as u64,
            )?;
        }
        Ok(())
    }

    fn handle_vmexit(
        &mut self,
        generic_vcpu_state: &mut GenericVCpuState,
    ) -> Result<VmexitResult, VmError> {
        let exit_reason = generic_vcpu_state.vmcs.exit_reason()?;
        let Self {
            pager,
            vmexit_controller,
            ..
        } = self;

        let r = pager.lock().try_lazy_paging(exit_reason);
        match r {
            Err(VmError::HandleVmexitFailed(exit_reason)) => vmexit_controller.handle(
                exit_reason,
                &mut pager::Probe { inner: pager },
                generic_vcpu_state,
            ),
            e => e,
        }
    }
}
