# Sign-in in the browser's memory alone

Unit U5.12 of the agent-setup run reads one question before any unit changes the start: can a run
keep a person's kept sign-in in a browser context that the browser keeps in memory, so that none of
it lies in the run's profile on disk. This file is that reading; no code of the product changed.

What ran. One hosted run of a scratch branch that is never merged: the program `r3-memory-probe`
of `tools/r3-trials/memory` at bdf226a5 on macos-15, with Google Chrome 158.0.8127.37
(`/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`) and Microsoft Edge 157.0.4341.60
(`/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge`), each once with a window and
once without. Each browser started as the executor starts it: the product's flags, agent settings,
relay and debugging pipe (`cdp::launch_args`, `start::start_on_pipe`), its profile at
`Library/Application Support/<folder>/agents/profiles/<cell>/profile`. Over that pipe the probe made
contexts with `Target.createBrowserContext`. It signed in at its own fixture on 127.0.0.1 in context
a: a 30-day cookie and one localStorage item, with markers that the probe made. Then it exported,
imported into context b with the product's `cookie_param` and `SessionState` form, opened the bill
page there, and made one more target in b with its address. A control signed in, with markers of
its own, in the profile's own context, which the product uses today. The probe searched every file
below the profile, its temporary directory, the run's folders under `Library/Caches` and the
browser's own new entry of the system's temporary directory: 40 s after the control's sign-in while
the browser lived, and again after the probe's own hard kill of the browser.

Choices. The probe does not call `Executor::start` and starts no guard: the executor has no public
call that sends a protocol command, and the reading changes nothing of the crate. Its client arms
each target as the product's wire does: auto-attach with the wait for the debugger, `Fetch.enable`
for every address, then the resume. The search counts the cookie's value and name and the storage
item's key and value, in UTF-8 and UTF-16LE. The browser keeps a cookie's value encrypted, so the
cookie's name is what shows a written cookie, as the control shows. The four cells share one
fixture, so the counts on the `rule` lines run over the whole run: each cell adds five requests at
the fixture, and one of them was not let through by a pause of the rule.

What the lines cannot show. One run of each cell, one version of each browser, one hosted image. The
search reads raw bytes and cannot see a marker in compressed bytes. Whether a target that the
profile's own context makes with its address misses the rule the same way was not read. Of other
programs' files the probe read names, kinds and sizes alone, and it did not read which program
wrote a changed file. The block leaves out 88 `library_new` lines of paths of Apple's services that
appeared during the headed cells and 443 `library_changed` lines; the job's log holds them, and for
Edge the probe listed 300 changed files and counted 50 more. The window pictures are told in words.

```text
hosted machine=hosted-mac run=45039366892 sha=bdf226a5080d60abb0de507b712f3a4d54c89f3c
probe start data=Library/Application Support/halvo-u512-memory-1310-1795361952960790000 fixture=127.0.0.1 relay_port=49181 marker_len=36
prefs file=com.google.Chrome.plist when=start stands=no
flags browser=chrome mode=headed count=26
start browser=chrome mode=headed product=Chrome/158.0.8127.37 pid=3012 started_us=1795361966979725
control browser=chrome mode=headed context=profile signin=ok
context browser=chrome mode=headed name=a made=yes proxy=inherited
signin browser=chrome mode=headed context=a signin=ok relay_tunnels_added=4 proxy=inherited
export browser=chrome mode=headed context=a with_context_id_cookie=yes with_context_id_storage=yes without_context_id_cookie=no control_cookie_in_profile_context=yes cookies_unparsed=0
import browser=chrome mode=headed context=b set_cookies=ok storage_set=yes landed_title=Bill signed_in=yes storage_had=yes
rule browser=chrome mode=headed contexts=a,b targets=7 page_targets=5 armed=5 waited_for_debugger=7 requests_at_fixture=5 bill_signed_in_at_fixture=2 unpaused_at_fixture=1 new_target_seen=yes new_target_paused_first=no blocked_failed=3 blocked_reached=0 late_attach=0 faults=0
window browser=chrome mode=headed which=memory source=cdp window_id=128049767 left=0 top=25 width=980 height=637 state=normal
window browser=chrome mode=headed which=control source=cdp window_id=128049761 left=22 top=47 width=980 height=637 state=normal
window browser=chrome mode=headed memory_and_control_share_a_window=no
window browser=chrome mode=headed which=memory page_title="Bill" headless_in_agent=no storage_quota=null
window browser=chrome mode=headed after_front=memory source=system number=40 owner=Google_Chrome title="Bill" layer=0 x=0 y=25 width=980 height=637
window browser=chrome mode=headed after_front=memory source=system number=35 owner=Google_Chrome title="Bill" layer=0 x=44 y=69 width=980 height=637
window browser=chrome mode=headed after_front=memory source=system number=23 owner=Google_Chrome title="Bill" layer=0 x=22 y=47 width=980 height=637
picture browser=chrome mode=headed which=memory number=40 png_bytes=34348 jpeg_base64_chars=36668 parts=13
window browser=chrome mode=headed after_front=control source=system number=23 owner=Google_Chrome title="Bill" layer=0 x=22 y=47 width=980 height=637
window browser=chrome mode=headed after_front=control source=system number=40 owner=Google_Chrome title="Bill" layer=0 x=0 y=25 width=980 height=637
window browser=chrome mode=headed after_front=control source=system number=35 owner=Google_Chrome title="Bill" layer=0 x=44 y=69 width=980 height=637
picture browser=chrome mode=headed which=control number=23 png_bytes=35454 jpeg_base64_chars=37312 parts=13
export browser=chrome mode=headed context=b at=run_end with_context_id_cookie=yes with_context_id_storage=yes without_context_id_cookie=no
disk browser=chrome mode=headed when=live files=167 bytes=8609968 unread=0 new_temp_entries=0 c_cookie_name=2 c_cookie_value=0 c_storage_key=1 c_storage_value=1 m_cookie_name=0 m_cookie_value=0 m_storage_key=0 m_storage_value=0
disk_place browser=chrome mode=headed when=live needle=c_cookie_name file=profile/Default/Cookies
disk_place browser=chrome mode=headed when=live needle=c_storage_key file=profile/Default/Local Storage/leveldb/000003.log
disk_place browser=chrome mode=headed when=live needle=c_storage_value file=profile/Default/Local Storage/leveldb/000003.log
end browser=chrome mode=headed way=hard kill_sent=yes tree=13 left=0 waited_ms=102
temp new_entries=11 browsers_own=1 others_counted_not_read=10
disk browser=chrome mode=headed when=after_hard_end files=168 bytes=8610079 unread=0 new_temp_entries=1 c_cookie_name=2 c_cookie_value=0 c_storage_key=1 c_storage_value=1 m_cookie_name=0 m_cookie_value=0 m_storage_key=0 m_storage_value=0
disk_place browser=chrome mode=headed when=after_hard_end needle=c_cookie_name file=profile/Default/Cookies
disk_place browser=chrome mode=headed when=after_hard_end needle=c_storage_key file=profile/Default/Local Storage/leveldb/000003.log
disk_place browser=chrome mode=headed when=after_hard_end needle=c_storage_value file=profile/Default/Local Storage/leveldb/000003.log
library browser=chrome mode=headed entries_before=342560 entries_after=342848 new_outside_profile=58 changed_files_outside_profile=93 gone=4 new_inside_profile=234 unread_dirs=0 cut=no walk_ms=9962
library_new browser=chrome mode=headed kind=dir bytes=96 path=Library/Application Support/Google
library_new browser=chrome mode=headed kind=dir bytes=128 path=Library/Application Support/Google/RLZ
library_new browser=chrome mode=headed kind=file bytes=181 path=Library/Application Support/Google/RLZ/RlzStore.plist
library_new browser=chrome mode=headed kind=file bytes=0 path=Library/Application Support/Google/RLZ/flockfile
library_new browser=chrome mode=headed kind=dir bytes=96 path=Library/Caches/halvo-u512-memory-1310-1795361952960790000
library_new browser=chrome mode=headed kind=dir bytes=96 path=Library/Caches/halvo-u512-memory-1310-1795361952960790000/agents
library_new browser=chrome mode=headed kind=dir bytes=96 path=Library/Caches/halvo-u512-memory-1310-1795361952960790000/agents/profiles
library_new browser=chrome mode=headed kind=dir bytes=96 path=Library/Caches/halvo-u512-memory-1310-1795361952960790000/agents/profiles/chrome-headed
library_new browser=chrome mode=headed kind=dir bytes=96 path=Library/Caches/halvo-u512-memory-1310-1795361952960790000/agents/profiles/chrome-headed/profile
library_new browser=chrome mode=headed kind=dir bytes=64 path=Library/Caches/halvo-u512-memory-1310-1795361952960790000/agents/profiles/chrome-headed/profile/Default
library_new browser=chrome mode=headed kind=file bytes=103 path=Library/Preferences/com.google.Chrome.plist
library_new browser=chrome mode=headed kind=file bytes=65 path=Library/Preferences/com.google.Keystone.Agent.plist
disk browser=chrome mode=headed when=library_outside_profile files=4 bytes=349 unread=0 c_cookie_name=0 c_cookie_value=0 c_storage_key=0 c_storage_value=0 m_cookie_name=0 m_cookie_value=0 m_storage_key=0 m_storage_value=0
cleanup browser=chrome mode=headed run_dir=removed
cleanup browser=chrome mode=headed cache_chain=removed
prefs file=com.google.Chrome.plist when=behind_chrome_headed stands=yes keys=1
prefs_diff from=start to=behind_chrome_headed stands_from=no stands_to=yes added=1 removed=0
prefs_key from=start to=behind_chrome_headed change=added key=LastRunAppBundlePath
flags browser=chrome mode=headless count=27
start browser=chrome mode=headless product=Chrome/158.0.8127.37 pid=4469 started_us=1795362039365281
control browser=chrome mode=headless context=profile signin=ok
context browser=chrome mode=headless name=a made=yes proxy=inherited
signin browser=chrome mode=headless context=a signin=ok relay_tunnels_added=4 proxy=inherited
export browser=chrome mode=headless context=a with_context_id_cookie=yes with_context_id_storage=yes without_context_id_cookie=no control_cookie_in_profile_context=yes cookies_unparsed=0
import browser=chrome mode=headless context=b set_cookies=ok storage_set=yes landed_title=Bill signed_in=yes storage_had=yes
rule browser=chrome mode=headless contexts=a,b targets=8 page_targets=5 armed=5 waited_for_debugger=8 requests_at_fixture=10 bill_signed_in_at_fixture=4 unpaused_at_fixture=2 new_target_seen=yes new_target_paused_first=no blocked_failed=3 blocked_reached=0 late_attach=0 faults=0
export browser=chrome mode=headless context=b at=run_end with_context_id_cookie=yes with_context_id_storage=yes without_context_id_cookie=no
disk browser=chrome mode=headless when=live files=163 bytes=8587612 unread=0 new_temp_entries=0 c_cookie_name=2 c_cookie_value=0 c_storage_key=1 c_storage_value=1 m_cookie_name=0 m_cookie_value=0 m_storage_key=0 m_storage_value=0
disk_place browser=chrome mode=headless when=live needle=c_cookie_name file=profile/Default/Cookies
disk_place browser=chrome mode=headless when=live needle=c_storage_key file=profile/Default/Local Storage/leveldb/000003.log
disk_place browser=chrome mode=headless when=live needle=c_storage_value file=profile/Default/Local Storage/leveldb/000003.log
end browser=chrome mode=headless way=hard kill_sent=yes tree=13 left=0 waited_ms=103
temp new_entries=3 browsers_own=1 others_counted_not_read=2
disk browser=chrome mode=headless when=after_hard_end files=163 bytes=8587612 unread=0 new_temp_entries=1 c_cookie_name=2 c_cookie_value=0 c_storage_key=1 c_storage_value=1 m_cookie_name=0 m_cookie_value=0 m_storage_key=0 m_storage_value=0
disk_place browser=chrome mode=headless when=after_hard_end needle=c_cookie_name file=profile/Default/Cookies
disk_place browser=chrome mode=headless when=after_hard_end needle=c_storage_key file=profile/Default/Local Storage/leveldb/000003.log
disk_place browser=chrome mode=headless when=after_hard_end needle=c_storage_value file=profile/Default/Local Storage/leveldb/000003.log
cleanup browser=chrome mode=headless run_dir=removed
cleanup browser=chrome mode=headless cache_chain=removed
flags browser=edge mode=headed count=26
start browser=edge mode=headed product=Edg/157.0.4341.60 pid=5665 started_us=1795362097705674
control browser=edge mode=headed context=profile signin=ok
context browser=edge mode=headed name=a made=yes proxy=inherited
signin browser=edge mode=headed context=a signin=ok relay_tunnels_added=4 proxy=inherited
export browser=edge mode=headed context=a with_context_id_cookie=yes with_context_id_storage=yes without_context_id_cookie=no control_cookie_in_profile_context=yes cookies_unparsed=0
import browser=edge mode=headed context=b set_cookies=ok storage_set=yes landed_title=Bill signed_in=yes storage_had=yes
rule browser=edge mode=headed contexts=a,b targets=5 page_targets=5 armed=5 waited_for_debugger=5 requests_at_fixture=15 bill_signed_in_at_fixture=6 unpaused_at_fixture=3 new_target_seen=yes new_target_paused_first=no blocked_failed=3 blocked_reached=0 late_attach=0 faults=0
window browser=edge mode=headed which=memory source=cdp window_id=816008153 left=0 top=25 width=980 height=640 state=normal
window browser=edge mode=headed which=control source=cdp window_id=816008136 left=22 top=47 width=980 height=640 state=normal
window browser=edge mode=headed memory_and_control_share_a_window=no
window browser=edge mode=headed which=memory page_title="Bill" headless_in_agent=no storage_quota=null
window browser=edge mode=headed after_front=memory source=system number=81 owner=Microsoft_Edge title="Bill" layer=0 x=0 y=25 width=980 height=640
window browser=edge mode=headed after_front=memory source=system number=75 owner=Microsoft_Edge title="Bill" layer=0 x=44 y=69 width=980 height=640
window browser=edge mode=headed after_front=memory source=system number=62 owner=Microsoft_Edge title="Bill" layer=0 x=22 y=47 width=980 height=640
picture browser=edge mode=headed which=memory number=81 png_bytes=34531 jpeg_base64_chars=41364 parts=14
window browser=edge mode=headed after_front=control source=system number=62 owner=Microsoft_Edge title="Bill" layer=0 x=22 y=47 width=980 height=640
window browser=edge mode=headed after_front=control source=system number=81 owner=Microsoft_Edge title="Bill" layer=0 x=0 y=25 width=980 height=640
window browser=edge mode=headed after_front=control source=system number=75 owner=Microsoft_Edge title="Bill" layer=0 x=44 y=69 width=980 height=640
picture browser=edge mode=headed which=control number=62 png_bytes=38167 jpeg_base64_chars=42876 parts=15
export browser=edge mode=headed context=b at=run_end with_context_id_cookie=yes with_context_id_storage=yes without_context_id_cookie=no
disk browser=edge mode=headed when=live files=196 bytes=13712931 unread=0 new_temp_entries=0 c_cookie_name=2 c_cookie_value=0 c_storage_key=1 c_storage_value=1 m_cookie_name=0 m_cookie_value=0 m_storage_key=0 m_storage_value=0
disk_place browser=edge mode=headed when=live needle=c_cookie_name file=profile/Default/Cookies
disk_place browser=edge mode=headed when=live needle=c_storage_key file=profile/Default/Local Storage/leveldb/000003.log
disk_place browser=edge mode=headed when=live needle=c_storage_value file=profile/Default/Local Storage/leveldb/000003.log
end browser=edge mode=headed way=hard kill_sent=yes tree=10 left=0 waited_ms=102
temp new_entries=5 browsers_own=1 others_counted_not_read=4
disk browser=edge mode=headed when=after_hard_end files=196 bytes=13712931 unread=0 new_temp_entries=1 c_cookie_name=2 c_cookie_value=0 c_storage_key=1 c_storage_value=1 m_cookie_name=0 m_cookie_value=0 m_storage_key=0 m_storage_value=0
disk_place browser=edge mode=headed when=after_hard_end needle=c_cookie_name file=profile/Default/Cookies
disk_place browser=edge mode=headed when=after_hard_end needle=c_storage_key file=profile/Default/Local Storage/leveldb/000003.log
disk_place browser=edge mode=headed when=after_hard_end needle=c_storage_value file=profile/Default/Local Storage/leveldb/000003.log
library browser=edge mode=headed entries_before=342611 entries_after=342973 new_outside_profile=96 changed_files_outside_profile=350 gone=5 new_inside_profile=271 unread_dirs=0 cut=no walk_ms=9620
library_new browser=edge mode=headed kind=dir bytes=64 path=Library/Application Scripts/UBF8T346G9.com.microsoft.edgemac.widgets
library_new browser=edge mode=headed kind=dir bytes=64 path=Library/Application Support/Microsoft Edge
library_new browser=edge mode=headed kind=dir bytes=288 path=Library/Application Support/Microsoft/EdgeUpdater
library_new browser=edge mode=headed kind=dir bytes=160 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0
library_new browser=edge mode=headed kind=dir bytes=256 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/Crashpad
library_new browser=edge mode=headed kind=dir bytes=64 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/Crashpad/attachments
library_new browser=edge mode=headed kind=dir bytes=64 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/Crashpad/completed
library_new browser=edge mode=headed kind=dir bytes=64 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/Crashpad/new
library_new browser=edge mode=headed kind=dir bytes=64 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/Crashpad/pending
library_new browser=edge mode=headed kind=file bytes=288 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/Crashpad/settings.dat
library_new browser=edge mode=headed kind=file bytes=20 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/Crashpad/throttle_store.dat
library_new browser=edge mode=headed kind=dir bytes=96 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app
library_new browser=edge mode=headed kind=dir bytes=256 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app/Contents
library_new browser=edge mode=headed kind=dir bytes=96 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app/Contents/Helpers
library_new browser=edge mode=headed kind=file bytes=152160 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app/Contents/Helpers/launcher
library_new browser=edge mode=headed kind=file bytes=1323 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app/Contents/Info.plist
library_new browser=edge mode=headed kind=dir bytes=128 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app/Contents/MacOS
library_new browser=edge mode=headed kind=file bytes=13076336 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app/Contents/MacOS/EdgeUpdater
library_new browser=edge mode=headed kind=file bytes=5701056 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app/Contents/MacOS/updater_oneds.so
library_new browser=edge mode=headed kind=file bytes=8 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app/Contents/PkgInfo
library_new browser=edge mode=headed kind=dir bytes=96 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app/Contents/Resources
library_new browser=edge mode=headed kind=file bytes=790982 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app/Contents/Resources/app.icns
library_new browser=edge mode=headed kind=dir bytes=96 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app/Contents/_CodeSignature
library_new browser=edge mode=headed kind=file bytes=3250 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/EdgeUpdater.app/Contents/_CodeSignature/CodeResources
library_new browser=edge mode=headed kind=file bytes=19 path=Library/Application Support/Microsoft/EdgeUpdater/147.0.3893.0/prefs.json
library_new browser=edge mode=headed kind=link bytes=12 path=Library/Application Support/Microsoft/EdgeUpdater/Current
library_new browser=edge mode=headed kind=file bytes=24576 path=Library/Application Support/Microsoft/EdgeUpdater/Diagnostic Data
library_new browser=edge mode=headed kind=dir bytes=96 path=Library/Application Support/Microsoft/EdgeUpdater/crx_cache
library_new browser=edge mode=headed kind=file bytes=13 path=Library/Application Support/Microsoft/EdgeUpdater/crx_cache/metadata.json
library_new browser=edge mode=headed kind=file bytes=440 path=Library/Application Support/Microsoft/EdgeUpdater/prefs.json
library_new browser=edge mode=headed kind=file bytes=36167 path=Library/Application Support/Microsoft/EdgeUpdater/updater.log
library_new browser=edge mode=headed kind=file bytes=8373 path=Library/Application Support/Microsoft/EdgeUpdater/updater_history.jsonl
library_new browser=edge mode=headed kind=dir bytes=192 path=Library/Caches/com.microsoft.EdgeUpdater
library_new browser=edge mode=headed kind=file bytes=49152 path=Library/Caches/com.microsoft.EdgeUpdater/Cache.db
library_new browser=edge mode=headed kind=file bytes=32768 path=Library/Caches/com.microsoft.EdgeUpdater/Cache.db-shm
library_new browser=edge mode=headed kind=file bytes=49472 path=Library/Caches/com.microsoft.EdgeUpdater/Cache.db-wal
library_new browser=edge mode=headed kind=dir bytes=64 path=Library/Caches/com.microsoft.EdgeUpdater/fsCachedData
library_new browser=edge mode=headed kind=dir bytes=96 path=Library/Caches/halvo-u512-memory-1310-1795361952960790000/agents/profiles/edge-headed
library_new browser=edge mode=headed kind=dir bytes=96 path=Library/Caches/halvo-u512-memory-1310-1795361952960790000/agents/profiles/edge-headed/profile
library_new browser=edge mode=headed kind=dir bytes=64 path=Library/Caches/halvo-u512-memory-1310-1795361952960790000/agents/profiles/edge-headed/profile/Default
library_new browser=edge mode=headed kind=dir bytes=128 path=Library/Group Containers/UBF8T346G9.com.microsoft.edgemac.widgets
library_new browser=edge mode=headed kind=file bytes=617 path=Library/Group Containers/UBF8T346G9.com.microsoft.edgemac.widgets/.com.apple.containermanagerd.metadata.plist
library_new browser=edge mode=headed kind=dir bytes=192 path=Library/Group Containers/UBF8T346G9.com.microsoft.edgemac.widgets/Library
library_new browser=edge mode=headed kind=dir bytes=96 path=Library/Group Containers/UBF8T346G9.com.microsoft.edgemac.widgets/Library/Application Scripts
library_new browser=edge mode=headed kind=link bytes=72 path=Library/Group Containers/UBF8T346G9.com.microsoft.edgemac.widgets/Library/Application Scripts/UBF8T346G9.com.microsoft.edgemac.widgets
library_new browser=edge mode=headed kind=dir bytes=64 path=Library/Group Containers/UBF8T346G9.com.microsoft.edgemac.widgets/Library/Application Support
library_new browser=edge mode=headed kind=dir bytes=64 path=Library/Group Containers/UBF8T346G9.com.microsoft.edgemac.widgets/Library/Caches
library_new browser=edge mode=headed kind=dir bytes=64 path=Library/Group Containers/UBF8T346G9.com.microsoft.edgemac.widgets/Library/Preferences
library_new browser=edge mode=headed kind=dir bytes=160 path=Library/HTTPStorages/com.microsoft.EdgeUpdater
library_new browser=edge mode=headed kind=file bytes=4096 path=Library/HTTPStorages/com.microsoft.EdgeUpdater/httpstorages.sqlite
library_new browser=edge mode=headed kind=file bytes=32768 path=Library/HTTPStorages/com.microsoft.EdgeUpdater/httpstorages.sqlite-shm
library_new browser=edge mode=headed kind=file bytes=20632 path=Library/HTTPStorages/com.microsoft.EdgeUpdater/httpstorages.sqlite-wal
library_new browser=edge mode=headed kind=file bytes=872 path=Library/LaunchAgents/com.microsoft.EdgeUpdater.wake.plist
library_new browser=edge mode=headed kind=file bytes=142 path=Library/Preferences/com.microsoft.edgemac.plist
disk browser=edge mode=headed when=library_outside_profile files=2 bytes=759 unread=0 c_cookie_name=0 c_cookie_value=0 c_storage_key=0 c_storage_value=0 m_cookie_name=0 m_cookie_value=0 m_storage_key=0 m_storage_value=0
cleanup browser=edge mode=headed run_dir=removed
cleanup browser=edge mode=headed cache_chain=removed
flags browser=edge mode=headless count=27
start browser=edge mode=headless product=Edg/157.0.4341.60 pid=7491 started_us=1795362164142412
control browser=edge mode=headless context=profile signin=ok
context browser=edge mode=headless name=a made=yes proxy=inherited
signin browser=edge mode=headless context=a signin=ok relay_tunnels_added=4 proxy=inherited
export browser=edge mode=headless context=a with_context_id_cookie=yes with_context_id_storage=yes without_context_id_cookie=no control_cookie_in_profile_context=yes cookies_unparsed=0
import browser=edge mode=headless context=b set_cookies=ok storage_set=yes landed_title=Bill signed_in=yes storage_had=yes
rule browser=edge mode=headless contexts=a,b targets=5 page_targets=5 armed=5 waited_for_debugger=5 requests_at_fixture=20 bill_signed_in_at_fixture=8 unpaused_at_fixture=4 new_target_seen=yes new_target_paused_first=no blocked_failed=3 blocked_reached=0 late_attach=0 faults=0
export browser=edge mode=headless context=b at=run_end with_context_id_cookie=yes with_context_id_storage=yes without_context_id_cookie=no
disk browser=edge mode=headless when=live files=195 bytes=9498118 unread=0 new_temp_entries=0 c_cookie_name=2 c_cookie_value=0 c_storage_key=1 c_storage_value=1 m_cookie_name=0 m_cookie_value=0 m_storage_key=0 m_storage_value=0
disk_place browser=edge mode=headless when=live needle=c_cookie_name file=profile/Default/Cookies
disk_place browser=edge mode=headless when=live needle=c_storage_key file=profile/Default/Local Storage/leveldb/000003.log
disk_place browser=edge mode=headless when=live needle=c_storage_value file=profile/Default/Local Storage/leveldb/000003.log
end browser=edge mode=headless way=hard kill_sent=yes tree=10 left=0 waited_ms=105
temp new_entries=1 browsers_own=1 others_counted_not_read=0
disk browser=edge mode=headless when=after_hard_end files=195 bytes=9498118 unread=0 new_temp_entries=1 c_cookie_name=2 c_cookie_value=0 c_storage_key=1 c_storage_value=1 m_cookie_name=0 m_cookie_value=0 m_storage_key=0 m_storage_value=0
disk_place browser=edge mode=headless when=after_hard_end needle=c_cookie_name file=profile/Default/Cookies
disk_place browser=edge mode=headless when=after_hard_end needle=c_storage_key file=profile/Default/Local Storage/leveldb/000003.log
disk_place browser=edge mode=headless when=after_hard_end needle=c_storage_value file=profile/Default/Local Storage/leveldb/000003.log
cleanup browser=edge mode=headless run_dir=removed
cleanup browser=edge mode=headless cache_chain=removed
prefs file=com.google.Chrome.plist when=end stands=yes keys=1
prefs_diff from=start to=end stands_from=no stands_to=yes added=1 removed=0
prefs_key from=start to=end change=added key=LastRunAppBundlePath
cleanup path=Library/Application Support/halvo-u512-memory-1310-1795361952960790000 removed
cleanup path=Library/Caches/halvo-u512-memory-1310-1795361952960790000 removed
probe end code=0
point 1 a_import=yes b_rule=partly c_export=yes_with_the_context_id: in all four cells context b took the kept sign-in in the product's cookie and storage form and its bill page opened signed in (import ... signed_in=yes storage_had=yes); every page target of the contexts was attached while it waited for the debugger and armed before its resume (page_targets=5 armed=5), a request to an origin outside the rule failed and reached nothing (blocked_failed=3 blocked_reached=0), and the relay was the contexts' route with no proxy named (relay_tunnels_added=4 proxy=inherited); but the target made in context b with its address sent its first request before the rule stood, in every cell (new_target_paused_first=no; unpaused_at_fixture rises by one a cell); the cookie and the item came back at the run's end by Storage.getCookies with the context's id and a storage page in the context (with_context_id_cookie=yes with_context_id_storage=yes), while the product's export call as written names no context and finds nothing of it (without_context_id_cookie=no)
point 2 memory_markers_on_disk=0: in all four cells, while the browser lived and after its hard end, no marker of the memory context stood in any file below the profile, its temporary directory, the run's cache folders or the browser's own temporary entry (m_cookie_name=0 m_cookie_value=0 m_storage_key=0 m_storage_value=0), nor in the browser's own files that a headed run made or changed under Library (when=library_outside_profile); the same search found the control's sign-in of the profile's own context in every cell, its cookie's name in profile/Default/Cookies and its storage item in profile/Default/Local Storage/leveldb/000003.log (c_cookie_name=2 c_storage_key=1 c_storage_value=1)
point 3 in plain words, from the system's list of the browser's windows and one picture of each of its two windows: the memory context opens in a window of its own beside the profile's window; it looks like any window of the browser, light, with no dark theme, no word Incognito or InPrivate and no private-window badge; the title that the system reads for it is the page's title alone (Bill); beside the profile's window it lacks the profile button at the right of the toolbar (Chrome: the person icon; Edge: the profile picture, and the Chat button too); both show the site's address and Not Secure for the fixture's plain http; nobody here decides what a person makes of it
point 4 behind a hard end of a headed run, outside the run's profile: Chrome left the empty chain of six folders Library/Caches/<folder>/agents/profiles/chrome-headed/profile/Default with no file, Library/Application Support/Google/RLZ with RlzStore.plist (181 bytes) and flockfile (0 bytes), Library/Preferences/com.google.Chrome.plist (103 bytes) and Library/Preferences/com.google.Keystone.Agent.plist (65 bytes), and in that time Library/Keychains/login.keychain-db, Library/Preferences/com.apple.dock.plist and Library/Preferences/com.apple.ncprefs.plist changed, by a writer that the probe did not read; Edge left the chain's three folders of its cell, Microsoft's updater installed for the account (Library/Application Support/Microsoft/EdgeUpdater with EdgeUpdater.app 147.0.3893.0, about 19.7 MB in 22 entries, Library/LaunchAgents/com.microsoft.EdgeUpdater.wake.plist of 872 bytes, and com.microsoft.EdgeUpdater under Library/Caches and Library/HTTPStorages), Library/Preferences/com.microsoft.edgemac.plist (142 bytes), the empty folder Library/Application Support/Microsoft Edge, and the folders of UBF8T346G9.com.microsoft.edgemac.widgets under Library/Group Containers and Library/Application Scripts; both headed runs saw a new saved-state folder in a daemon's container, whose owner the probe did not read; no marker stood in any of the browser's files among them
point 5 Library/Preferences/com.google.Chrome.plist did not stand before the first Chrome run (when=start stands=no); behind the headed Chrome run it stood with one key name, LastRunAppBundlePath, added and none removed, and at the probe's end, after the headless Chrome run too, the same one key
verdict memory=partly open=b_target_made_with_its_address browsers=chrome,edge modes=headed,headless
```
