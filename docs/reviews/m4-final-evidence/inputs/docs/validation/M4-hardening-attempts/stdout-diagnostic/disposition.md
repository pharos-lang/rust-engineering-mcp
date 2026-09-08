# Stdout observer diagnostic

Core attempt 2 failed the preexisting UnixStream shutdown observer assertion before it sent the request that checks server exit. The old assertion discarded whether the channel returned Timeout, an I/O error or a frame. No mechanism can therefore be established from that failure.

The test now reports the variant, elapsed time and bootstrap case, and reports only a frame length. Its 10-second deadline and strict requirement for Disconnected are unchanged. No error or frame is newly accepted and no product code changed.

Thirty isolated repetitions passed. Five repetitions of the full 44-test protocol suite passed (220 tests). A separate benign socket probe observed EOF in 2000 cases. The full suite also takes about 11.1 seconds when it passes, so the failed suite duration is not evidence for a stdout timeout. Concurrent client load is only an unproven hypothesis. This is not a resolution of the separate inherited M3 Linux CLI observation.

Technical Owner follow-up: if it recurs, use the new diagnostic to distinguish I/O, output and scheduling before altering the oracle. The original failed core receipt/log are preserved in core-attempt-2. Final core/full gates remain mandatory.
