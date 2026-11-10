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
    env.with_env(|env| -> Result<(), jni::errors::Error> {
        env.find_class(jni_str!("org/rustls/platformverifier/CertificateVerifier"))?;
        rustls_platform_verifier::android::init_with_env(env, context)?;
        READY.store(true, Ordering::Release);
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}
