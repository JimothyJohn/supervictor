pub fn config_esp() {
    esp_println::logger::init_logger_from_env();
    // TODO: Optimize this once able
    esp_alloc::heap_allocator!(size: 72 * 1024);
}
