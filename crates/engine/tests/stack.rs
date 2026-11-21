//! The engine is built on the stack of whichever thread builds it: the main thread of the
//! window, 1 MiB on Windows, which also runs the window system. A debug build of the
//! application overflowed it once the decks held the key locks of the stems.

use platter_core::SampleRate;
use platter_engine::Engine;

#[test]
fn the_engine_is_built_and_dropped_within_half_a_mebibyte_of_stack() {
    let built = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let (engine, processor) = Engine::new(SampleRate::HZ_48000);
            let snapshot = engine.snapshot();
            drop((snapshot, processor, engine));
        })
        .map(|thread| thread.join().is_ok());
    assert_eq!(built.ok(), Some(true));
}
