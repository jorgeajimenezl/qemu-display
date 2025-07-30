# QEMU -display dbus library

[![Crates.io](https://img.shields.io/crates/v/qemu-display.svg)](https://crates.io/crates/qemu-display)
[![Documentation](https://docs.rs/qemu-display/badge.svg)](https://docs.rs/qemu-display)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Provides convenient APIs to communicate with `qemu -display dbus` from Rust.

## Features

- provides all common D-Bus interfaces by default
- provides "unix" and "win32" specific interfaces on respective targets
- optional "qmp" feature, to allow connecting to QEMU via p2p / bus-less.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
qemu-display = "0.2"
```

## Quick Start

Here's a simple example of how to use the library:

```rust,no_run
# use std::error::Error;
use qemu_display::Display;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let conn = zbus::Connection::session().await?;
    let display = Display::new::<()>(&conn, None).await?;
    // TODO: complete this example
    Ok(())
}
```

## API Documentation

For detailed API documentation, please visit [docs.rs/qemu-display](https://docs.rs/qemu-display).

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request. For major changes, please open an issue first to discuss what you would like to change.

Please make sure to update tests as appropriate.

### Development Setup

1. Clone the repository:
```bash
git clone https://gitlab.com/marcandre.lureau/qemu-display
cd qemu-display/qemu-display
```

2. Build the project:
```bash
cargo build
```

3. Run tests:
```bash
cargo test
```

## Changelog

### [0.1.0] - 2025-01-15
- Initial release

## License

This project is licensed under the MIT License - see the [LICENSE](https://opensource.org/licenses/MIT) file for details.

## Acknowledgments

- Thanks to contributors
- Credit to Rust, QEMU and zbus
- Red Hat!

## Contact

- Email: marcandre.lureau@gmail.com

---
Built with ❤️ using Rust
