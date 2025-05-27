//! Implementation of [`PageTableEntry`] and [`PageTable`].

use crate::config::PAGE_SIZE;

use super::{frame_alloc, FrameTracker, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
use alloc::vec;
use alloc::vec::Vec;
use bitflags::*;

bitflags! {
    /// page table entry flags
    pub struct PTEFlags: u8 {
        /// Valid
        const V = 1 << 0;
        /// Readable
        const R = 1 << 1;
        /// Writable
        const W = 1 << 2;
        /// eXecutable
        const X = 1 << 3;
        /// User
        const U = 1 << 4;
        /// Global
        const G = 1 << 5;
        /// Accessed
        const A = 1 << 6;
        /// Dirty
        const D = 1 << 7;
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
/// page table entry structure
pub struct PageTableEntry {
    /// bits of page table entry
    pub bits: usize,
}

impl PageTableEntry {
    /// Create a new page table entry
    pub fn new(ppn: PhysPageNum, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: ppn.0 << 10 | flags.bits as usize,
        }
    }
    /// Create an empty page table entry
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }
    /// Get the physical page number from the page table entry
    pub fn ppn(&self) -> PhysPageNum {
        (self.bits >> 10 & ((1usize << 44) - 1)).into()
    }
    /// Get the flags from the page table entry
    pub fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits(self.bits as u8).unwrap()
    }
    /// The page pointered by page table entry is valid?
    pub fn is_valid(&self) -> bool {
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is readable?
    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is writable?
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is executable?
    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is user?
    pub fn is_user(&self) -> bool {
        (self.flags() & PTEFlags::U) != PTEFlags::empty()
    }
}

/// page table structure
pub struct PageTable {
    root_ppn: PhysPageNum,
    frames: Vec<FrameTracker>,
}

/// Assume that it won't oom when creating/mapping.
impl PageTable {
    /// Create a new page table
    pub fn new() -> Self {
        let frame = frame_alloc().unwrap();
        PageTable {
            root_ppn: frame.ppn,
            frames: vec![frame],
        }
    }
    /// Temporarily used to get arguments from user space.
    pub fn from_token(satp: usize) -> Self {
        Self {
            root_ppn: PhysPageNum::from(satp & ((1usize << 44) - 1)),
            frames: Vec::new(),
        }
    }
    /// Find PageTableEntry by VirtPageNum, create a frame for a 4KB page table if not exist
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                let frame = frame_alloc().unwrap();
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V);
                self.frames.push(frame);
            }
            ppn = pte.ppn();
        }
        result
    }
    /// Find PageTableEntry by VirtPageNum
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if !pte.is_valid() {
                return None;
            }
            if i == 2 {
                result = Some(pte);
                break;
            }
            ppn = pte.ppn();
        }
        result
    }
    /// set the map between virtual page number and physical page number
    #[allow(unused)]
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags) {
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V);
    }
    /// remove the map between virtual page number and physical page number
    #[allow(unused)]
    pub fn unmap(&mut self, vpn: VirtPageNum) {
        let pte = self.find_pte(vpn).unwrap();
        assert!(pte.is_valid(), "vpn {:?} is invalid before unmapping", vpn);
        *pte = PageTableEntry::empty();
    }
    /// get the page table entry from the virtual page number
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.find_pte(vpn).map(|pte| *pte)
    }
    /// get the token from the page table
    pub fn token(&self) -> usize {
        8usize << 60 | self.root_ppn.0
    }
}

/// Translate&Copy a ptr[u8] array with LENGTH len to a mutable u8 Vec through page table
pub fn translated_byte_buffer(token: usize, ptr: *const u8, len: usize) -> Vec<&'static mut [u8]> {
    let page_table = PageTable::from_token(token);
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::new();
    while start < end {
        let start_va = VirtAddr::from(start);
        let mut vpn = start_va.floor();
        let ppn = page_table.translate(vpn).unwrap().ppn();
        vpn.step();
        let mut end_va: VirtAddr = vpn.into();
        end_va = end_va.min(VirtAddr::from(end));
        if end_va.page_offset() == 0 {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..]);
        } else {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..end_va.page_offset()]);
        }
        start = end_va.into();
    }
    v
}

/// 拷贝一个buf到用户空间的ptr[u8]数组
pub fn copy_to_user(token: usize, ptr: *mut u8, buf: *mut u8, len: usize) -> isize {
    let page_table = PageTable::from_token(token);
    let mut start = ptr as usize;
    let mut buf_offset = 0;
    let end = start + len;
    while start < end {
        let start_va = VirtAddr::from(start);
        let vpn = start_va.floor();
        let ppn = page_table.translate(vpn).unwrap().ppn();
        let page_offset = start_va.page_offset();
        let bytes_left_in_page = PAGE_SIZE - page_offset;
        let bytes_left_total = end - start;
        let copy_len = bytes_left_in_page.min(bytes_left_total);
        ppn.get_bytes_array()[page_offset..page_offset + copy_len]
            .copy_from_slice(unsafe { core::slice::from_raw_parts(buf.add(buf_offset), copy_len) });
        start += copy_len;
        buf_offset += copy_len;
    }
    0
}

/// read from user space address
pub fn read_user_addr(token: usize, addr: usize) -> Result<u8, ()> {
    let page_table = PageTable::from_token(token);
    let vpn = VirtAddr::from(addr).floor();
    if let Some(pte) = page_table.translate(vpn) {
        if pte.is_user() && pte.is_valid() && pte.readable() {
            debug!("read user addr: {:#x} -> {:#x}", addr, pte.ppn().get_bytes_array()[VirtAddr::from(addr).page_offset()]);
            debug!("pte flags: {:#x}", pte.flags().bits());
            debug!("pte ppn: {:#x}", pte.ppn().0);
            debug!("pte bits: {:#x}", pte.bits);
            Ok(pte.ppn().get_bytes_array()[VirtAddr::from(addr).page_offset()])
        } else {
            debug!("read user addr: {:#x} -> invalid", addr);
            Err(())
        }
    } else {
        Err(())
    }
}

/// write to user space address
pub fn write_user_addr(token: usize, addr: usize, value: u8) -> Result<(), ()> {
    let page_table = PageTable::from_token(token);
    let vpn = VirtAddr::from(addr).floor();
    if let Some(pte) = page_table.translate(vpn) {
        if pte.is_user() && pte.is_valid() && pte.writable() {
            pte.ppn().get_bytes_array()[VirtAddr::from(addr).page_offset()] = value;
            Ok(())
        } else {
            Err(())
        }
    } else {
        Err(())
    }
}
