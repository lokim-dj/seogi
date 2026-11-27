// JNI bridge from the phone app's dictation to transcribe.cpp
// (MIT, Copyright (c) 2026 The transcribe.cpp authors).
// One session per handle; the Kotlin side uses a handle from one thread at a time.
#include <jni.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <android/log.h>
#include "transcribe.h"

#define TAG "SeogiAsr"

static void on_log(transcribe_log_level level, const char *msg, void *ud) {
    (void)ud;
    if (level != TRANSCRIBE_LOG_LEVEL_ERROR && level != TRANSCRIBE_LOG_LEVEL_WARN) return;
    __android_log_write(level == TRANSCRIBE_LOG_LEVEL_ERROR ? ANDROID_LOG_ERROR : ANDROID_LOG_WARN, TAG, msg);
}

// UTF-8 bytes, decoded on the Kotlin side (NewStringUTF wants modified UTF-8).
static jbyteArray to_bytes(JNIEnv *env, const char *text) {
    jsize n = (jsize)strlen(text);
    jbyteArray out = (*env)->NewByteArray(env, n);
    if (out) (*env)->SetByteArrayRegion(env, out, 0, n, (const jbyte *)text);
    return out;
}

JNIEXPORT jlong JNICALL
Java_io_github_lokimdj_seogi_dictation_TranscribeCpp_nativeOpen(JNIEnv *env, jclass cls, jstring jpath, jint threads) {
    (void)cls;
    static int ready = 0;
    if (!ready) { transcribe_log_set(on_log, NULL); transcribe_init_backends_default(); ready = 1; }
    const char *path = (*env)->GetStringUTFChars(env, jpath, NULL);
    if (!path) return 0;
    struct transcribe_model_load_params lp;
    transcribe_model_load_params_init(&lp);
    lp.backend = TRANSCRIBE_BACKEND_CPU;
    struct transcribe_session_params sp;
    transcribe_session_params_init(&sp);
    sp.n_threads = threads;
    struct transcribe_session *s = NULL;
    transcribe_status st = transcribe_open(path, &lp, &sp, &s);
    (*env)->ReleaseStringUTFChars(env, jpath, path);
    if (st != TRANSCRIBE_OK || !s) {
        __android_log_print(ANDROID_LOG_ERROR, TAG, "open failed: %s", transcribe_status_string(st));
        return 0;
    }
    return (jlong)(intptr_t)s;
}

// The text as UTF-8, or "\x01" followed by the error.
JNIEXPORT jbyteArray JNICALL
Java_io_github_lokimdj_seogi_dictation_TranscribeCpp_nativeRun(JNIEnv *env, jclass cls, jlong h, jfloatArray jpcm, jstring jlang) {
    (void)cls;
    struct transcribe_session *s = (struct transcribe_session *)(intptr_t)h;
    if (!s) return to_bytes(env, "\x01no session");
    jsize n = (*env)->GetArrayLength(env, jpcm);
    jfloat *pcm = (*env)->GetFloatArrayElements(env, jpcm, NULL);
    if (!pcm) return to_bytes(env, "\x01out of memory");
    const char *lang = jlang ? (*env)->GetStringUTFChars(env, jlang, NULL) : NULL;
    struct transcribe_run_params rp;
    transcribe_run_params_init(&rp);
    rp.language = lang;
    transcribe_status st = transcribe_run(s, pcm, (int)n, &rp);
    (*env)->ReleaseFloatArrayElements(env, jpcm, pcm, JNI_ABORT);
    if (lang) (*env)->ReleaseStringUTFChars(env, jlang, lang);
    if (st != TRANSCRIBE_OK) {
        char buf[160];
        snprintf(buf, sizeof buf, "\x01%s", transcribe_status_string(st));
        return to_bytes(env, buf);
    }
    const char *text = transcribe_full_text(s);
    return to_bytes(env, text ? text : "");
}

JNIEXPORT void JNICALL
Java_io_github_lokimdj_seogi_dictation_TranscribeCpp_nativeClose(JNIEnv *env, jclass cls, jlong h) {
    (void)env; (void)cls;
    if (h) transcribe_session_free((struct transcribe_session *)(intptr_t)h);
}
