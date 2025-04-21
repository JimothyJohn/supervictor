![rustacean](docs/banner.jpg)

# supervictor

An experiment in deploying Rust on a RISC-V MCU. If you can do it here you can do it anywhere!

## Goal

Extract information from the environment as efficiently as possible.

### Hardware

- [XIAO ESP32C3](https://wiki.seeedstudio.com/XIAO_ESP32C3_Getting_Started/)


### Built on

- esp-generate 0.3.0

### Resources

- [The Rust Programming Language](https://doc.rust-lang.org/book/)
    - [Bookmark](https://doc.rust-lang.org/book/ch03-05-control-flow.html)

- [The Embedded Rust Book](https://docs.rust-embedded.org/book/index.html)
    - [Bookmark](https://docs.rust-embedded.org/book/start/qemu.html)

- [The Rust on ESP Book](https://docs.esp-rs.org/book/)

- [Embedded Rust (no_std) on Espressif](https://docs.esp-rs.org/no_std-training/)
    - [Bookmark](https://docs.esp-rs.org/no_std-training/03_6_http_client.html)

- [Embassy Book](https://embassy.dev/book/index.html)

- [Impl Rust for ESP32](https://esp32.implrust.com)

### TODO

- [x] [Create an async GET HTTP request](examples/wifi_embassy_dhcp.rs)

- [x] [Create an async POST HTTPS request with JSON request/response](examples/tls_dns_dhcp_wifi.rs)

- [ ] Connect to AWS IoT over MQTT [Reference](https://github.com/sambenko/esp32s3-no-std-async-tls-mqtt/blob/main/src/main.rs)

- [ ] Utilize flash/NVS encryption. [Reference](https://espressif.github.io/esp32-c3-book-en/chapter_13/13.3/13.3.7.html)
