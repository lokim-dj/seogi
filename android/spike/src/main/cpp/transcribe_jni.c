// JNI bridge from the spike app to transcribe.cpp (MIT, The transcribe.cpp authors).
#include <jni.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <android/log.h>
#include "transcribe.h"

#define TAG "SeogiAsr"

static void on_log(transcribe_log_level level, const char *msg, void *ud) {
    (void)ud;
    int prio = level == TRANSCRIBE_LOG_LEVEL_ERROR ? ANDROID_LOG_ERROR
             : level == TRANSCRIBE_LOG_LEVEL_WARN  ? ANDROID_LOG_WARN : ANDROID_LOG_INFO;
    __android_log_write(prio, TAG, msg);
}

// UTF-8 bytes, decoded on the Kotlin side (NewStringUTF wants modified UTF-8).
static jbyteArray to_bytes(JNIEnv *env, const char *text) {
    jsize n = (jsize)strlen(text);
    jbyteArray out = (*env)->NewByteArray(env, n);
    (*env)->SetByteArrayRegion(env, out, 0, n, (const jbyte *)text);
    return out;
}

JNIEXPORT jlong JNICALL
Java_io_github_lokimdj_seogi_spike_TranscribeCpp_nativeOpen(JNIEnv *env, jclass cls, jstring jpath, jint threads) {
    (void)cls;
    static int logging = 0;
    if (!logging) { transcribe_log_set(on_log, NULL); transcribe_init_backends_default(); logging = 1; }
    const char *path = (*env)->GetStringUTFChars(env, jpath, NULL);
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

JNIEXPORT jbyteArray JNICALL
Java_io_github_lokimdj_seogi_spike_TranscribeCpp_nativeRun(JNIEnv *env, jclass cls, jlong h, jfloatArray jpcm, jstring jlang) {
    (void)cls;
    struct transcribe_session *s = (struct transcribe_session *)(intptr_t)h;
    jsize n = (*env)->GetArrayLength(env, jpcm);
    jfloat *pcm = (*env)->GetFloatArrayElements(env, jpcm, NULL);
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
Java_io_github_lokimdj_seogi_spike_TranscribeCpp_nativeClose(JNIEnv *env, jclass cls, jlong h) {
    (void)env; (void)cls;
    transcribe_session_free((struct transcribe_session *)(intptr_t)h);
}
