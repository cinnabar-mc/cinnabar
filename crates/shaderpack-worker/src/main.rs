//! The isolated shader-pack compiler process the client spawns for CPU translation.

#[global_allocator]
static ALLOCATOR: shaderpack::worker::BudgetAllocator<std::alloc::System> =
    shaderpack::worker::BudgetAllocator(std::alloc::System);

/// Installs the compiler's limits and translates the request supplied on stdin.
fn main() {
    std::process::exit(shaderpack::worker::run_stdio());
}
