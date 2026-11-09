//! iOS app entry; elsewhere this program does nothing, so desktop builds still compile it.

#[cfg(target_os = "ios")]
mod ios;

// Keep live heap accounting available to the cooperative in-app shader compiler.
#[cfg(target_os = "ios")]
#[global_allocator]
static ALLOCATOR: shaderpack::worker::BudgetAllocator<std::alloc::System> =
    shaderpack::worker::BudgetAllocator(std::alloc::System);

fn main() {
    #[cfg(target_os = "ios")]
    ios::main();
}
