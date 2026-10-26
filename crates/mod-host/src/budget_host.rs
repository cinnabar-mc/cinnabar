//! The ungated `budget` interface: a callback reads its own remaining fuel. Linked by hand
//! because generated host traits cannot reach the calling store, which holds the fuel.

use wasmtime::component::{HasData, Linker};

/// The interface's import name, as the WIT package declares it; the budget guest test fails
/// to link when they differ.
pub(crate) const INTERFACE: &str = "cinnabar:extension/budget@0.1.0";

/// Holds no host state: `remaining-fuel` reads the calling store.
pub trait Host {}
impl<T: ?Sized> Host for T {}

/// The store-access bound generated world linkers require of every interface.
pub trait HostWithStore: HasData {}
impl<T: ?Sized + HasData> HostWithStore for T {}

/// Defines `remaining-fuel`, the fuel the calling store has left.
pub fn add_to_linker<T: 'static, D>(
    linker: &mut Linker<T>,
    _host_getter: fn(&mut T) -> D::Data<'_>,
) -> wasmtime::Result<()>
where
    D: HostWithStore,
    for<'a> D::Data<'a>: Host,
{
    linker.instance(INTERFACE)?.func_wrap(
        "remaining-fuel",
        |store: wasmtime::StoreContextMut<'_, T>, (): ()| Ok((store.get_fuel()?,)),
    )
}
