//! (d) `unsafe impl Send` gerado por `macro_rules!` de outra crate, pra um tipo com ponteiro cru (que
//! sem isso não seria `Send`).

pub struct RawHandle(pub *const u8);

unsafe_macros::impl_send!(RawHandle);

fn require_send<T: Send>() {}

pub fn handle_is_send() {
    require_send::<RawHandle>();
}
