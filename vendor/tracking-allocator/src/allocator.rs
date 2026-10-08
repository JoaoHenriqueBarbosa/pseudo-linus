use std::alloc::{handle_alloc_error, GlobalAlloc, Layout, System};

use crate::token::try_with_suspended_allocation_group;
use crate::{get_global_tracker, AllocationGroupId};

/// Tracking allocator implementation.
///
/// This allocator must be installed via `#[global_allocator]` in order to take effect.  More
/// information on using this allocator can be found in the examples, or directly in the standard
/// library docs for [`GlobalAlloc`].
pub struct Allocator<A> {
    inner: A,
}

impl<A> Allocator<A> {
    /// Creates a new `Allocator` that wraps another allocator.
    #[must_use]
    pub const fn from_allocator(allocator: A) -> Self {
        Self { inner: allocator }
    }
}

impl Allocator<System> {
    /// Creates a new `Allocator` that wraps the system allocator.
    #[must_use]
    pub const fn system() -> Allocator<System> {
        Self::from_allocator(System)
    }
}

impl<A: GlobalAlloc> Allocator<A> {
    unsafe fn get_wrapped_allocation(
        &self,
        object_layout: Layout,
    ) -> (*mut usize, *mut u8, Layout) {
        // Allocate our wrapped layout and make sure the allocation succeeded.
        let (actual_layout, offset_to_object) = get_wrapped_layout(object_layout);
        let actual_ptr = self.inner.alloc(actual_layout);
        if actual_ptr.is_null() {
            handle_alloc_error(actual_layout);
        }

        // Zero out the group ID field to make sure it's in the `None` state.
        //
        // SAFETY: We know that `actual_ptr` is at least aligned enough for casting it to `*mut usize` as the layout for
        // the allocation backing this pointer ensures the first field in the layout is `usize.
        #[allow(clippy::cast_ptr_alignment)]
        let group_id_ptr = actual_ptr.cast::<usize>();
        group_id_ptr.write(0);

        // SAFETY: If the allocation succeeded and `actual_ptr` is valid, then it must be valid to advance by
        // `offset_to_object` as it would land within the allocation.
        let object_ptr = actual_ptr.wrapping_add(offset_to_object);

        (group_id_ptr, object_ptr, actual_layout)
    }
}

impl Default for Allocator<System> {
    fn default() -> Self {
        Self::from_allocator(System)
    }
}

unsafe impl<A: GlobalAlloc> GlobalAlloc for Allocator<A> {
    #[track_caller]
    unsafe fn alloc(&self, object_layout: Layout) -> *mut u8 {
        let (group_id_ptr, object_ptr, wrapped_layout) = self.get_wrapped_allocation(object_layout);
        let object_addr = object_ptr as usize;
        let object_size = object_layout.size();
        let wrapped_size = wrapped_layout.size();

        if let Some(tracker) = get_global_tracker() {
            try_with_suspended_allocation_group(
                #[inline(always)]
                |group_id| {
                    // We only set the group ID in the wrapper header if we're tracking an allocation, because when it
                    // comes back to us during deallocation, we want to skip doing any checks at all if it's already
                    // zero.
                    //
                    // If we never track the allocation, tracking the deallocation will only produce incorrect numbers,
                    // and that includes even if we just used the rule of "always attribute allocations to the root
                    // allocation group by default".
                    group_id_ptr.write(group_id.as_usize().get());
                    tracker.allocated(object_addr, object_size, wrapped_size, group_id);
                },
            );
        }

        object_ptr
    }

    // pseudo-linus: o `realloc` padrão aloca, copia e libera sempre. Com o rastreio desligado o alocador de
    // baixo redimensiona no lugar (o mimalloc quase nunca move), com o cabeçalho zerado de novo como o
    // `alloc` deixa; ligado, vale o caminho padrão, que conta a alocação nova e a liberação da velha.
    #[track_caller]
    unsafe fn realloc(&self, object_ptr: *mut u8, object_layout: Layout, new_size: usize) -> *mut u8 {
        let new_layout = Layout::from_size_align_unchecked(new_size, object_layout.align());
        if get_global_tracker().is_some() {
            let new_ptr = self.alloc(new_layout);
            if !new_ptr.is_null() {
                std::ptr::copy_nonoverlapping(object_ptr, new_ptr, object_layout.size().min(new_size));
                self.dealloc(object_ptr, object_layout);
            }
            return new_ptr;
        }
        let (old_wrapped, offset_to_object) = get_wrapped_layout(object_layout);
        let (new_wrapped, _) = get_wrapped_layout(new_layout);
        let actual_ptr = object_ptr.wrapping_sub(offset_to_object);
        let new_actual = self.inner.realloc(actual_ptr, old_wrapped, new_wrapped.size());
        if new_actual.is_null() {
            handle_alloc_error(new_wrapped);
        }
        #[allow(clippy::cast_ptr_alignment)]
        new_actual.cast::<usize>().write(0);
        new_actual.wrapping_add(offset_to_object)
    }

    #[track_caller]
    unsafe fn dealloc(&self, object_ptr: *mut u8, object_layout: Layout) {
        // Regenerate the wrapped layout so we know where we have to look, as the pointer we've given relates to the
        // requested layout, not the wrapped layout that was actually allocated.
        let (wrapped_layout, offset_to_object) = get_wrapped_layout(object_layout);

        // SAFETY: We only ever return pointers to the actual requested object layout, not our wrapped layout. Since
        // global allocators cannot be changed at runtime, we know that if we're here, then the given pointer, and the
        // allocation it refers to, was allocated by us. Thus, since we wrap _all_ allocations, we know that this object
        // pointer can be safely subtracted by `offset_to_object` to get back to the group ID field in our wrapper.
        let actual_ptr = object_ptr.wrapping_sub(offset_to_object);

        // pseudo-linus: com o rastreio desligado (o estado de quase toda a vida do processo) o cabeçalho não
        // é lido: a liberação é só a do alocador de baixo. O custo do rastreio passa a existir só quando ele
        // está ligado.
        let Some(tracker) = get_global_tracker() else {
            self.inner.dealloc(actual_ptr, wrapped_layout);
            return;
        };

        // SAFETY: We know that `actual_ptr` is at least aligned enough for casting it to `*mut usize` as the layout for
        // the allocation backing this pointer ensures the first field in the layout is `usize.
        #[allow(clippy::cast_ptr_alignment)]
        let raw_group_id = actual_ptr.cast::<usize>().read();

        // Deallocate before tracking, just to make sure we're reclaiming memory as soon as possible.
        self.inner.dealloc(actual_ptr, wrapped_layout);

        let object_addr = object_ptr as usize;
        let object_size = object_layout.size();
        let wrapped_size = wrapped_layout.size();

        if let Some(source_group_id) = AllocationGroupId::from_raw(raw_group_id) {
            try_with_suspended_allocation_group(
                #[inline(always)]
                |current_group_id| {
                    tracker.deallocated(
                        object_addr,
                        object_size,
                        wrapped_size,
                        source_group_id,
                        current_group_id,
                    );
                },
            );
        }
    }
}

#[inline(always)]
fn get_wrapped_layout(object_layout: Layout) -> (Layout, usize) {
    // We generate a new allocation layout that gives us a location to store the active allocation group ID ahead
    // of the requested allocation, which lets us always attempt to retrieve it on the deallocation path. We'll
    // always set this to zero, and conditionally update it to the actual allocation group ID if tracking is enabled.
    // pseudo-linus: o objeto sai alinhado a pelo menos 16, como o malloc entrega. Com o cabeçalho de
    // 8 bytes na frente, um pedido de alinhamento 8 caía em endereço 8 mod 16, e o libunwind do LLVM
    // (o unwinder do alvo musl) grava a `_Unwind_Exception` do Rust com `movaps`: SIGSEGV em todo panic.
    //
    // Conta direta, equivalente a `align_to(16)` + `Layout::new::<usize>().extend(..)` + `pad_to_align()`: o
    // alinhamento é `max(16, pedido)` (potência de dois, então múltiplo do cabeçalho de 8), o objeto começa
    // nesse mesmo deslocamento e o tamanho total sobe ao múltiplo do alinhamento. Sem as três chamadas de
    // `Layout` com `expect` por alocação e por liberação.
    let align = if object_layout.align() > 16 { object_layout.align() } else { 16 };
    let size = object_layout
        .size()
        .checked_add(align)
        .and_then(|total| total.checked_add(align - 1))
        .map(|total| total & !(align - 1))
        .expect("wrapping requested layout resulted in overflow");
    let actual_layout = Layout::from_size_align(size, align).expect("wrapping requested layout resulted in overflow");

    (actual_layout, align)
}

