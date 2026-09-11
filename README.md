# rust-gpu-driver-lab
## What this is
This is a Rust implementation of a simulated GPU driver that manages buffer allocation, command submission, and device state. It's an educational/prototype codebase demonstrating driver architecture patterns including device state machines, buffer management, command validation, and FIFO queue processing.
## stack
- Language: Rust (100%)
- Framework / runtime: Standard library only (no external dependencies)
- Notable patterns: Type-safe validation, error handling via Result/custom enums, trait-based abstraction for queue backends

## How it's organized
```bash
src/
  lib.rs                    Core GPU device & buffer management (GpuDevice, BufferId)
  
  driver.rs                 Concrete SimulatedGpuDriver implementation
  generic_driver.rs         Generic driver parameterized over queue backend
  typed_driver.rs           Typed driver with state-based APIs
  typed_generic_driver.rs   Combination of generic + typed patterns
  
  device_state.rs           Device state machine (Offline → FirmwareLoaded → Ready)
                            DeviceController manages firmware validation & transitions
  
  command.rs                Command validation layer
                            CommandId, Priority, CommandRange with strict validation
  
  submission.rs             Submission validation
                            Bridges RawCommand with device buffers
  
  queue.rs                  CommandQueue (dynamic FIFO, dedup IDs)
  queue_backend.rs          SubmissionQueue trait (abstraction)
  static_queue.rs           StaticCommandQueue (compile-time sized, dedup IDs)
  
  registers.rs              MMIO register definitions
  mmio.rs                   Memory-mapped I/O operations
  
  submission_session.rs     Session tracking for submissions
```

The driver follows a layered validation pipeline:

1. Device State Layer (device_state.rs): Firmware load → device startup. Must reach Ready state before any operations.

2. Buffer Management Layer (lib.rs): GpuDevice holds allocated buffers as GpuBuffer (with separate storage and used tracking). Buffers have immutable IDs and capacity constraints.

3. Command Validation Layer (command.rs): Raw commands are validated for non-zero ID, priority bounds (0–3), 4-byte alignment, and no range overflow.

4. Submission Validation Layer (submission.rs): Validated commands are paired with buffers. The submission ensures the command's byte range doesn't exceed initialized data.

5. Queue Abstraction (queue.rs, queue_backend.rs, static_queue.rs): CommandQueue (dynamic) and StaticCommandQueue (compile-time sized) both implement SubmissionQueue trait and track active command IDs to prevent duplicates.

6. Driver Facade (driver.rs, generic_driver.rs): High-level API that orchestrates state checks, buffer ops, and queue enqueuing. Generic driver allows queue backend to be plugged in.

## How to Run it
```bash
# Run all tests (no external dependencies needed)
cargo test

# Build the library
cargo build

# Run tests with output
cargo test -- --nocapture

# Check for errors
cargo check
```
