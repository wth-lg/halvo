cells: private.macos-15.chrome private.macos-15.edge private.macos-15.firefox private.macos-15.safari private.windows-2025.chrome private.windows-2025.edge private.windows-2025.firefox
hosted-run branch=scratch/r2-probes-1 commit=cdd5ec6e96f84371a4cd5545094a502fedf77cc1 run=42889376963 attempt=1
cell private.macos-15.chrome runs=3 signal=attribute version=157.0.8103.40 attribute_hit=descendant.AXButton.AXDescription attribute_runs=3 class_runs=0 title_runs=3 start_reruns_count=0
cell private.macos-15.edge runs=3 signal=attribute version=156.0.4314.58 attribute_hit=descendant.AXButton.AXDescription attribute_runs=3 class_runs=0 title_runs=3 start_reruns_count=0
cell private.macos-15.firefox runs=3 signal=title version=159.0.1 attribute_hit=none attribute_runs=0 class_runs=0 title_runs=3 start_reruns_count=0
cell private.macos-15.safari runs=3 signal=attribute version=27.2 attribute_hit=descendant.AXStaticText.AXValue attribute_runs=3 class_runs=0 title_runs=3 start_reruns_count=0
cell private.windows-2025.chrome runs=3 signal=attribute version=156.0.8078.80 attribute_hit=descendant.50000.Name attribute_runs=3 class_runs=0 title_runs=0 start_reruns_count=0
cell private.windows-2025.edge runs=3 signal=attribute version=156.0.4314.58 attribute_hit=descendant.50000.Name attribute_runs=3 class_runs=0 title_runs=3 start_reruns_count=0
cell private.windows-2025.firefox runs=3 signal=attribute version=159.0.1 attribute_hit=descendant.50000.ClassName attribute_runs=3 class_runs=0 title_runs=3 start_reruns_count=0
command cells of a hosted run: sh tools/r2-probes/run-hosted.sh OUT
note version: Chrome on macos-15 updated itself during run 42889376963, so its meta.json names 157.0.8103.39 and the cell private.macos-15.chrome names 157.0.8103.40, which the probe read from the build on disk when it listed the browsers before the private-window cells; the figures stand as measured, and the probe now records in each run the build on disk when that run started the browser, for a later run
command this file: r2-probes trial private-window --artifact MACOS --artifact WINDOWS [--records THIS-MAC] --lines LINES --out FILE
verdict private-window chrome macos-15: readable
verdict private-window chrome windows-2025: readable
verdict private-window edge macos-15: readable
verdict private-window edge windows-2025: readable
verdict private-window firefox macos-15: readable
verdict private-window firefox windows-2025: readable
verdict private-window safari macos-15: readable
