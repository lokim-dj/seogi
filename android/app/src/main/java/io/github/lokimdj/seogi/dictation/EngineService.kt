package io.github.lokimdj.seogi.dictation

import android.app.Service
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Binder
import android.os.IBinder
import android.os.Parcel
import android.os.Process
import android.os.RemoteException
import android.os.SharedMemory
import android.system.OsConstants
import android.util.Log
import java.nio.ByteOrder
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * The speech engine in a process of its own (`:engine`), apart from the key
 * and the accessibility service in `:dictation` (#98). The engine is native
 * code holding about 1.4 GB: when it crashes, or Android ends it for memory,
 * only this process goes, and the accessibility service the user switched on
 * stays connected. Two crashes of one process within two minutes make Android
 * stop restarting its services (AppErrors, "crashed too many times"), and an
 * accessibility service Android stops that way stays "not working" until the
 * user switches it off and on.
 *
 * Bound by [RemoteEngine] only (not exported). The model opens on the first
 * call and closes when the last client lets go ([onUnbind]).
 */
class EngineService : Service() {
    private val lock = Any()
    private var engine: SpeechEngine? = null

    private val binder = object : Binder() {
        override fun onTransact(code: Int, data: Parcel, reply: Parcel?, flags: Int): Boolean {
            if (code != WARM && code != RUN) return super.onTransact(code, data, reply, flags)
            if (Binder.getCallingUid() != Process.myUid()) return false
            data.enforceInterface(DESCRIPTOR)
            val answer = try {
                val pcm = if (code == RUN) readPcm(data) else null
                synchronized(lock) {
                    val e = engine ?: SpeechEngine.open(DictationModel.file(this@EngineService)).also { engine = it }
                    if (pcm == null) "" else e.transcribe(pcm)
                }.let { Result.success(it) }
            } catch (t: Throwable) {
                Log.w(TAG, "the engine failed", t)
                Result.failure(t)
            }
            reply?.writeInt(if (answer.isSuccess) 1 else 0)
            reply?.writeString(answer.getOrNull() ?: answer.exceptionOrNull()?.message.orEmpty())
            return true
        }
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onUnbind(intent: Intent?): Boolean {
        closeEngine()
        return false
    }

    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        // Nobody is waiting on it while no call runs; [RemoteEngine] opens it again.
        if (level >= TRIM_MEMORY_RUNNING_LOW) closeEngine()
    }

    override fun onDestroy() {
        closeEngine()
        super.onDestroy()
    }

    private fun closeEngine() {
        synchronized(lock) {
            engine?.close()
            engine = null
        }
    }

    companion object {
        private const val TAG = "SeogiDictation"
        internal const val DESCRIPTOR = "io.github.lokimdj.seogi.dictation.Engine"
        internal const val WARM = IBinder.FIRST_CALL_TRANSACTION
        internal const val RUN = IBinder.FIRST_CALL_TRANSACTION + 1

        /** The sound comes in shared memory: a 25 s piece is 1.6 MB, more than a binder call carries. */
        private fun readPcm(data: Parcel): FloatArray {
            val count = data.readInt()
            SharedMemory.CREATOR.createFromParcel(data).use { mem ->
                val buf = mem.mapReadOnly()
                try {
                    val out = FloatArray(count)
                    buf.order(ByteOrder.nativeOrder()).asFloatBuffer().get(out)
                    return out
                } finally {
                    SharedMemory.unmap(buf)
                }
            }
        }
    }
}

/** The engine's process ended under a call: the next use binds again. */
class EngineGone(message: String, cause: Throwable? = null) : IllegalStateException(message, cause)

/**
 * The key's side of [EngineService]: bound for as long as it is open. Not
 * thread-safe, like [SpeechEngine]: used from the key's engine thread only,
 * never the main thread (opening waits for Android to bind).
 */
class RemoteEngine private constructor(private val context: Context) : AutoCloseable {
    @Volatile private var binder: IBinder? = null
    @Volatile private var gone = false
    private val connected = CountDownLatch(1)

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            binder = service
            connected.countDown()
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            // The engine's process ended (a crash, or Android needed the memory).
            gone = true
            binder = null
        }

        override fun onBindingDied(name: ComponentName?) {
            gone = true
            binder = null
            connected.countDown()
        }
    }

    /** The text of [pcm] (16 kHz mono floats), or throws with the engine's reason. */
    fun transcribe(pcm: FloatArray): String {
        val mem = SharedMemory.create("seogi-pcm", maxOf(1, pcm.size) * 4)
        try {
            val buf = mem.mapReadWrite()
            try {
                buf.order(ByteOrder.nativeOrder()).asFloatBuffer().put(pcm)
            } finally {
                SharedMemory.unmap(buf)
            }
            mem.setProtect(OsConstants.PROT_READ)
            return call(EngineService.RUN) { p ->
                p.writeInt(pcm.size)
                mem.writeToParcel(p, 0)
            }
        } finally {
            mem.close()
        }
    }

    private fun warm() {
        call(EngineService.WARM) {}
    }

    private fun call(code: Int, write: (Parcel) -> Unit): String {
        val b = binder
        if (gone || b == null) throw EngineGone("the engine is not running")
        val data = Parcel.obtain()
        val reply = Parcel.obtain()
        try {
            data.writeInterfaceToken(EngineService.DESCRIPTOR)
            write(data)
            b.transact(code, data, reply, 0)
            val ok = reply.readInt() == 1
            val text = reply.readString().orEmpty()
            if (!ok) throw IllegalStateException(text)
            return text
        } catch (e: RemoteException) {
            gone = true
            throw EngineGone("the engine ended", e)
        } finally {
            data.recycle()
            reply.recycle()
        }
    }

    override fun close() {
        try { context.unbindService(connection) } catch (_: Exception) {}
        binder = null
    }

    companion object {
        private const val CONNECT_MS = 10_000L

        /** Binds the engine and opens the model in it: slow (a second or two). Not on the main thread. */
        fun open(context: Context): RemoteEngine {
            val r = RemoteEngine(context.applicationContext)
            val ok = try {
                context.applicationContext.bindService(
                    Intent(context, EngineService::class.java), r.connection, Context.BIND_AUTO_CREATE,
                )
            } catch (e: Exception) {
                false
            }
            if (!ok) {
                r.close()
                throw EngineGone("could not bind the engine")
            }
            if (!r.connected.await(CONNECT_MS, TimeUnit.MILLISECONDS) || r.binder == null) {
                r.close()
                throw EngineGone("the engine did not start")
            }
            try {
                r.warm()
            } catch (e: Throwable) {
                r.close()
                throw e
            }
            return r
        }
    }
}
