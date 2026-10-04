//! Android application initialization only; no protocol or persistence logic.
use std::sync::atomic::{AtomicBool, Ordering};

use jni::{
    EnvUnowned, jni_str,
    objects::{JClass, JObject},
};

static READY: AtomicBool = AtomicBool::new(false);

pub(crate) fn ready() -> Result<(), String> {
    if READY.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("initialize the Android application context before opening a controller".into())
    }
}

#[unsafe(export_name = "Java_ai_sailry_bridge_Native_init")]
pub extern "system" fn initialize<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    context: JObject<'local>,
) {
    // jni-rs catches panics and translates failures to Java exceptions. Neither
    // JNI local references nor an unwind may escape this native invocation.
    env.with_env(|env| -> Result<(), jni::errors::Error> {
        // Fail during setup if the companion AAR was stripped or not packaged.
        env.find_class(jni_str!("org/rustls/platformverifier/CertificateVerifier"))?;
        rustls_platform_verifier::android::init_with_env(env, context)?;
        READY.store(true, Ordering::Release);
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}
