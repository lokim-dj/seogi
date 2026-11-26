package io.github.lokimdj.seogi

import io.github.lokimdj.seogi.record.Space
import org.junit.Assert.assertEquals
import org.junit.Test

/** Review 5: room to record and to save. */
class SpaceTest {
    private val mb = Space.MB

    @Test fun startsOnlyWithRoomForAThreeHourMeetingAndItsSave() {
        assertEquals(false, Space.canStart(199 * mb))
        assertEquals(true, Space.canStart(200 * mb))
    }

    @Test fun stopsBeforeThereIsNoRoomToSave() {
        assertEquals(false, Space.mustStop(500 * mb, 80 * mb))
        assertEquals(true, Space.mustStop(85 * mb, 80 * mb)) // the save needs another 80 MB
        assertEquals(true, Space.mustStop(30 * mb, 1 * mb)) // below the floor
    }
}
