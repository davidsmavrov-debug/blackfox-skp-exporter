//! What authoring takes from its surroundings: the save time a written
//! file records and the entropy behind the GUIDs and per-file bytes it
//! carries (`docs/WRITING.md`). By default both come from the system; a
//! caller with no system clock (WebAssembly in a browser, where
//! `std::time::SystemTime::now` panics) or one that wants a reproducible
//! file supplies them through [`WriteEnv`].

use std::cell::Cell;

/// The save time and seed one authoring call uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteEnv {
    /// Seconds since the UNIX epoch, recorded as the save time.
    pub unix_time: u32,
    /// Seeds the GUIDs and per-file header bytes; two files written with
    /// the same seed and time from the same model are identical.
    pub seed: u64,
}

impl WriteEnv {
    /// The system clock and a fresh seed. On a target without a clock
    /// (`wasm32-unknown-unknown`) the time is 0 and the seed a counter.
    pub fn system() -> WriteEnv {
        WriteEnv {
            unix_time: system_secs(),
            seed: system_seed(),
        }
    }
}

thread_local! {
    /// The environment of the authoring call in progress on this thread:
    /// the time, and the generator state the seed starts.
    static CURRENT: Cell<Option<(u32, u64)>> = const { Cell::new(None) };
}

/// Run `f` with `env` as the authoring environment, restoring whatever was
/// in force before (so a nested call cannot leak its environment).
pub(crate) fn with_env<T>(env: WriteEnv, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<(u32, u64)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(CURRENT.with(|c| c.replace(Some((env.unix_time, env.seed)))));
    f()
}

/// Four bytes of entropy: the next output of the call's generator when an
/// environment is in force, else the system's.
pub(crate) fn random_bytes() -> [u8; 4] {
    let from_env = CURRENT.with(|c| {
        c.get().map(|(t, state)| {
            let (next, out) = splitmix64(state);
            c.set(Some((t, next)));
            out
        })
    });
    let v = from_env.unwrap_or_else(system_seed);
    ((v >> 32) as u32 ^ v as u32).to_le_bytes()
}

/// The save time: the environment's, else the system clock's.
pub(crate) fn now_secs() -> u32 {
    CURRENT
        .with(|c| c.get().map(|(t, _)| t))
        .unwrap_or_else(system_secs)
}

fn splitmix64(state: u64) -> (u64, u64) {
    let next = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = next;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (next, z ^ (z >> 31))
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn system_secs() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn system_secs() -> u32 {
    0
}

/// The OS entropy source (via the address-space and time mix the standard
/// library exposes without a dependency).
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn system_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.finish()
}

/// No entropy source: a counter through the generator, so successive
/// files still differ.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn system_seed() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0x5EED);
    splitmix64(COUNTER.fetch_add(1, Ordering::Relaxed)).1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_environment_makes_the_entropy_reproducible() {
        let env = WriteEnv {
            unix_time: 1_700_000_000,
            seed: 42,
        };
        let a = with_env(env, || (random_bytes(), random_bytes(), now_secs()));
        let b = with_env(env, || (random_bytes(), random_bytes(), now_secs()));
        assert_eq!(a, b);
        assert_ne!(a.0, a.1, "successive draws differ");
        assert_eq!(a.2, 1_700_000_000);
    }

    #[test]
    fn the_environment_ends_with_the_call() {
        with_env(
            WriteEnv {
                unix_time: 7,
                seed: 1,
            },
            || {
                with_env(
                    WriteEnv {
                        unix_time: 9,
                        seed: 2,
                    },
                    || assert_eq!(now_secs(), 9),
                );
                assert_eq!(now_secs(), 7);
            },
        );
        assert!(CURRENT.with(|c| c.get()).is_none());
    }
}
