reading trial=window machine=mac arm=a2 system=macos call=background_throttling value=suspend
reading trial=window machine=mac arm=a2 system=windows call=set_is_visible value=false
reading trial=window machine=mac arm=a2 system=windows call=set_memory_usage_target_level value=low
reading trial=window machine=mac arm=a2 system=macos expect=same_as_a source=wry-0.57.1 policy_set=only_when_named
reading trial=window machine=mac figure=key_bytes system=macos command=ps
reading trial=window machine=mac figure=second_bytes system=macos command=proc_pid_rusage
reading trial=window machine=mac figure=cpu_ms system=macos command=ps
reading trial=window machine=mac figure=key_bytes system=windows command=Get-Process
reading trial=window machine=mac figure=second_bytes system=windows command=Get-Process
reading trial=window machine=mac figure=cpu_ms system=windows command=Get-Process
cell trial=window machine=hosted-mac arm=main run=1 key_bytes=124420096 second_bytes=39179392 helpers=2 cpu_ms=580 ticks=330 frames=0 load_x100=89 cpu_pct=- counted=yes show_ms=42
cell trial=window machine=hosted-mac arm=main run=2 key_bytes=122044416 second_bytes=38016320 helpers=2 cpu_ms=580 ticks=330 frames=0 load_x100=104 cpu_pct=- counted=yes show_ms=34
cell trial=window machine=hosted-mac arm=main run=3 key_bytes=122880000 second_bytes=38376704 helpers=2 cpu_ms=560 ticks=330 frames=0 load_x100=60 cpu_pct=- counted=yes show_ms=43
cell trial=window machine=hosted-mac arm=main run=4 key_bytes=122994688 second_bytes=38212800 helpers=2 cpu_ms=560 ticks=330 frames=0 load_x100=105 cpu_pct=- counted=yes show_ms=49
cell trial=window machine=hosted-mac arm=main run=5 key_bytes=122732544 second_bytes=37868736 helpers=2 cpu_ms=680 ticks=330 frames=0 load_x100=95 cpu_pct=- counted=yes show_ms=52
cell trial=window machine=hosted-mac arm=main run=6 key_bytes=122765312 second_bytes=38229376 helpers=2 cpu_ms=440 ticks=331 frames=0 load_x100=149 cpu_pct=- counted=yes show_ms=49
cell trial=window machine=hosted-mac arm=main run=7 key_bytes=122077184 second_bytes=38622656 helpers=2 cpu_ms=540 ticks=330 frames=0 load_x100=83 cpu_pct=- counted=yes show_ms=28
cell trial=window machine=hosted-mac arm=main run=8 key_bytes=122585088 second_bytes=38032640 helpers=2 cpu_ms=560 ticks=331 frames=0 load_x100=56 cpu_pct=- counted=yes show_ms=23
cell trial=window machine=hosted-mac arm=main run=9 key_bytes=123420672 second_bytes=38524160 helpers=2 cpu_ms=400 ticks=330 frames=0 load_x100=34 cpu_pct=- counted=yes show_ms=43
cell trial=window machine=hosted-mac arm=main run=10 key_bytes=122683392 second_bytes=38098176 helpers=2 cpu_ms=370 ticks=331 frames=0 load_x100=53 cpu_pct=- counted=yes show_ms=35
cell trial=window machine=hosted-mac arm=a run=1 key_bytes=124567552 second_bytes=39638336 helpers=2 cpu_ms=110 ticks=0 frames=0 load_x100=42 cpu_pct=- counted=yes show_ms=40
cell trial=window machine=hosted-mac arm=a run=2 key_bytes=122716160 second_bytes=38737216 helpers=2 cpu_ms=90 ticks=0 frames=0 load_x100=63 cpu_pct=- counted=yes show_ms=30
cell trial=window machine=hosted-mac arm=a run=3 key_bytes=122306560 second_bytes=38491392 helpers=2 cpu_ms=120 ticks=0 frames=0 load_x100=88 cpu_pct=- counted=yes show_ms=32
cell trial=window machine=hosted-mac arm=a run=4 key_bytes=122519552 second_bytes=38540480 helpers=2 cpu_ms=110 ticks=0 frames=0 load_x100=81 cpu_pct=- counted=yes show_ms=47
cell trial=window machine=hosted-mac arm=a run=5 key_bytes=122814464 second_bytes=38475072 helpers=2 cpu_ms=100 ticks=0 frames=0 load_x100=74 cpu_pct=- counted=yes show_ms=28
cell trial=window machine=hosted-mac arm=a run=6 key_bytes=122961920 second_bytes=38261888 helpers=2 cpu_ms=120 ticks=0 frames=0 load_x100=61 cpu_pct=- counted=yes show_ms=15
cell trial=window machine=hosted-mac arm=a run=7 key_bytes=122667008 second_bytes=38376512 helpers=2 cpu_ms=110 ticks=0 frames=0 load_x100=82 cpu_pct=- counted=yes show_ms=21
cell trial=window machine=hosted-mac arm=a run=8 key_bytes=122781696 second_bytes=38491328 helpers=2 cpu_ms=90 ticks=0 frames=0 load_x100=115 cpu_pct=- counted=yes show_ms=33
cell trial=window machine=hosted-mac arm=a run=9 key_bytes=122142720 second_bytes=38261952 helpers=2 cpu_ms=90 ticks=0 frames=0 load_x100=63 cpu_pct=- counted=yes show_ms=16
cell trial=window machine=hosted-mac arm=a run=10 key_bytes=122667008 second_bytes=38262016 helpers=2 cpu_ms=120 ticks=0 frames=0 load_x100=39 cpu_pct=- counted=yes show_ms=25
cell trial=window machine=hosted-mac arm=a2 run=1 key_bytes=124567552 second_bytes=39163072 helpers=2 cpu_ms=120 ticks=0 frames=0 load_x100=71 cpu_pct=- counted=yes show_ms=22
cell trial=window machine=hosted-mac arm=a2 run=2 key_bytes=123518976 second_bytes=39540032 helpers=2 cpu_ms=80 ticks=0 frames=0 load_x100=38 cpu_pct=- counted=yes show_ms=25
cell trial=window machine=hosted-mac arm=a2 run=3 key_bytes=123158528 second_bytes=38442048 helpers=2 cpu_ms=90 ticks=0 frames=0 load_x100=50 cpu_pct=- counted=yes show_ms=19
cell trial=window machine=hosted-mac arm=a2 run=4 key_bytes=122044416 second_bytes=38278464 helpers=2 cpu_ms=100 ticks=0 frames=0 load_x100=102 cpu_pct=- counted=yes show_ms=32
cell trial=window machine=hosted-mac arm=a2 run=5 key_bytes=122699776 second_bytes=38278400 helpers=2 cpu_ms=110 ticks=0 frames=0 load_x100=71 cpu_pct=- counted=yes show_ms=24
cell trial=window machine=hosted-mac arm=a2 run=6 key_bytes=122732544 second_bytes=38524032 helpers=2 cpu_ms=80 ticks=0 frames=0 load_x100=81 cpu_pct=- counted=yes show_ms=31
cell trial=window machine=hosted-mac arm=a2 run=7 key_bytes=123125760 second_bytes=38098048 helpers=2 cpu_ms=140 ticks=0 frames=0 load_x100=90 cpu_pct=- counted=yes show_ms=33
cell trial=window machine=hosted-mac arm=a2 run=8 key_bytes=122699776 second_bytes=38393024 helpers=2 cpu_ms=100 ticks=0 frames=0 load_x100=112 cpu_pct=- counted=yes show_ms=26
cell trial=window machine=hosted-mac arm=a2 run=9 key_bytes=122535936 second_bytes=38458688 helpers=2 cpu_ms=90 ticks=0 frames=0 load_x100=37 cpu_pct=- counted=yes show_ms=20
cell trial=window machine=hosted-mac arm=a2 run=10 key_bytes=122257408 second_bytes=38409344 helpers=2 cpu_ms=100 ticks=0 frames=0 load_x100=38 cpu_pct=- counted=yes show_ms=32
cell trial=window machine=hosted-mac arm=b run=1 key_bytes=87687168 second_bytes=20532480 helpers=1 cpu_ms=40 ticks=- frames=- load_x100=58 cpu_pct=- counted=yes show_ms=1349
cell trial=window machine=hosted-mac arm=b run=2 key_bytes=86163456 second_bytes=19287488 helpers=1 cpu_ms=50 ticks=- frames=- load_x100=55 cpu_pct=- counted=yes show_ms=1397
cell trial=window machine=hosted-mac arm=b run=3 key_bytes=85737472 second_bytes=19385664 helpers=1 cpu_ms=30 ticks=- frames=- load_x100=89 cpu_pct=- counted=yes show_ms=1297
cell trial=window machine=hosted-mac arm=b run=4 key_bytes=86278144 second_bytes=19713600 helpers=1 cpu_ms=40 ticks=- frames=- load_x100=51 cpu_pct=- counted=yes show_ms=1300
cell trial=window machine=hosted-mac arm=b run=5 key_bytes=85688320 second_bytes=19369344 helpers=1 cpu_ms=50 ticks=- frames=- load_x100=85 cpu_pct=- counted=yes show_ms=1296
cell trial=window machine=hosted-mac arm=b run=6 key_bytes=86016000 second_bytes=19205376 helpers=1 cpu_ms=50 ticks=- frames=- load_x100=91 cpu_pct=- counted=yes show_ms=1200
cell trial=window machine=hosted-mac arm=b run=7 key_bytes=86081536 second_bytes=19320000 helpers=1 cpu_ms=50 ticks=- frames=- load_x100=78 cpu_pct=- counted=yes show_ms=1314
cell trial=window machine=hosted-mac arm=b run=8 key_bytes=85622784 second_bytes=19369344 helpers=1 cpu_ms=40 ticks=- frames=- load_x100=83 cpu_pct=- counted=yes show_ms=1426
cell trial=window machine=hosted-mac arm=b run=9 key_bytes=86343680 second_bytes=19893696 helpers=1 cpu_ms=60 ticks=- frames=- load_x100=107 cpu_pct=- counted=yes show_ms=1287
cell trial=window machine=hosted-mac arm=b run=10 key_bytes=85835776 second_bytes=19418560 helpers=1 cpu_ms=30 ticks=- frames=- load_x100=97 cpu_pct=- counted=yes show_ms=1318
cell trial=window machine=hosted-win arm=main run=1 key_bytes=146055168 second_bytes=103837696 helpers=6 cpu_ms=12248 ticks=3423 frames=21910 load_x100=- cpu_pct=4 counted=yes show_ms=41
cell trial=window machine=hosted-win arm=main run=2 key_bytes=146833408 second_bytes=107708416 helpers=6 cpu_ms=12140 ticks=3424 frames=21913 load_x100=- cpu_pct=4 counted=yes show_ms=26
cell trial=window machine=hosted-win arm=main run=3 key_bytes=148533248 second_bytes=105037824 helpers=6 cpu_ms=12515 ticks=3423 frames=21909 load_x100=- cpu_pct=4 counted=yes show_ms=17
cell trial=window machine=hosted-win arm=main run=4 key_bytes=147542016 second_bytes=107241472 helpers=6 cpu_ms=12468 ticks=3423 frames=21906 load_x100=- cpu_pct=4 counted=yes show_ms=13
cell trial=window machine=hosted-win arm=main run=5 key_bytes=145268736 second_bytes=104574976 helpers=6 cpu_ms=12920 ticks=3423 frames=21908 load_x100=- cpu_pct=4 counted=yes show_ms=21
cell trial=window machine=hosted-win arm=main run=6 key_bytes=145252352 second_bytes=103538688 helpers=6 cpu_ms=11968 ticks=3423 frames=21910 load_x100=- cpu_pct=5 counted=yes show_ms=20
cell trial=window machine=hosted-win arm=main run=7 key_bytes=145784832 second_bytes=104640512 helpers=6 cpu_ms=12641 ticks=3425 frames=21916 load_x100=- cpu_pct=5 counted=yes show_ms=20
cell trial=window machine=hosted-win arm=main run=8 key_bytes=146206720 second_bytes=105664512 helpers=6 cpu_ms=11422 ticks=3424 frames=21916 load_x100=- cpu_pct=4 counted=yes show_ms=25
cell trial=window machine=hosted-win arm=main run=9 key_bytes=146612224 second_bytes=104034304 helpers=6 cpu_ms=12968 ticks=3424 frames=21914 load_x100=- cpu_pct=4 counted=yes show_ms=23
cell trial=window machine=hosted-win arm=main run=10 key_bytes=145334272 second_bytes=103665664 helpers=6 cpu_ms=12391 ticks=3423 frames=21911 load_x100=- cpu_pct=4 counted=yes show_ms=21
cell trial=window machine=hosted-win arm=a run=1 key_bytes=135716864 second_bytes=95408128 helpers=6 cpu_ms=219 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=49
cell trial=window machine=hosted-win arm=a run=2 key_bytes=136237056 second_bytes=94842880 helpers=6 cpu_ms=203 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=18
cell trial=window machine=hosted-win arm=a run=3 key_bytes=141918208 second_bytes=96550912 helpers=6 cpu_ms=296 ticks=0 frames=0 load_x100=- cpu_pct=74 counted=no show_ms=44
cell trial=window machine=hosted-win arm=a run=4 key_bytes=135741440 second_bytes=95834112 helpers=6 cpu_ms=251 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=14
cell trial=window machine=hosted-win arm=a run=5 key_bytes=136224768 second_bytes=95776768 helpers=6 cpu_ms=78 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=25
cell trial=window machine=hosted-win arm=a run=6 key_bytes=135675904 second_bytes=98750464 helpers=6 cpu_ms=110 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=27
cell trial=window machine=hosted-win arm=a run=7 key_bytes=136073216 second_bytes=96403456 helpers=6 cpu_ms=202 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=13
cell trial=window machine=hosted-win arm=a run=8 key_bytes=135831552 second_bytes=96759808 helpers=6 cpu_ms=235 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=15
cell trial=window machine=hosted-win arm=a run=9 key_bytes=136065024 second_bytes=99340288 helpers=6 cpu_ms=265 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=21
cell trial=window machine=hosted-win arm=a run=10 key_bytes=135479296 second_bytes=97886208 helpers=6 cpu_ms=186 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=22
cell trial=window machine=hosted-win arm=a2 run=1 key_bytes=106315776 second_bytes=98144256 helpers=6 cpu_ms=155 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=46
cell trial=window machine=hosted-win arm=a2 run=2 key_bytes=106934272 second_bytes=98181120 helpers=6 cpu_ms=281 ticks=0 frames=0 load_x100=- cpu_pct=5 counted=yes show_ms=30
cell trial=window machine=hosted-win arm=a2 run=3 key_bytes=107196416 second_bytes=97910784 helpers=6 cpu_ms=280 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=26
cell trial=window machine=hosted-win arm=a2 run=4 key_bytes=106893312 second_bytes=97648640 helpers=6 cpu_ms=171 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=25
cell trial=window machine=hosted-win arm=a2 run=5 key_bytes=106217472 second_bytes=94703616 helpers=6 cpu_ms=218 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=32
cell trial=window machine=hosted-win arm=a2 run=6 key_bytes=106102784 second_bytes=94752768 helpers=6 cpu_ms=235 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=29
cell trial=window machine=hosted-win arm=a2 run=7 key_bytes=106151936 second_bytes=94666752 helpers=6 cpu_ms=282 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=29
cell trial=window machine=hosted-win arm=a2 run=8 key_bytes=106348544 second_bytes=97611776 helpers=6 cpu_ms=375 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=29
cell trial=window machine=hosted-win arm=a2 run=9 key_bytes=106012672 second_bytes=94838784 helpers=6 cpu_ms=250 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=26
cell trial=window machine=hosted-win arm=a2 run=10 key_bytes=107520000 second_bytes=98111488 helpers=6 cpu_ms=359 ticks=0 frames=0 load_x100=- cpu_pct=3 counted=yes show_ms=24
cell trial=window machine=hosted-win arm=b run=1 key_bytes=20041728 second_bytes=4337664 helpers=0 cpu_ms=16 ticks=- frames=- load_x100=- cpu_pct=3 counted=yes show_ms=478
cell trial=window machine=hosted-win arm=b run=2 key_bytes=18628608 second_bytes=4145152 helpers=0 cpu_ms=0 ticks=- frames=- load_x100=- cpu_pct=3 counted=yes show_ms=464
cell trial=window machine=hosted-win arm=b run=3 key_bytes=18690048 second_bytes=4190208 helpers=0 cpu_ms=0 ticks=- frames=- load_x100=- cpu_pct=3 counted=yes show_ms=442
cell trial=window machine=hosted-win arm=b run=4 key_bytes=19148800 second_bytes=4247552 helpers=0 cpu_ms=0 ticks=- frames=- load_x100=- cpu_pct=3 counted=yes show_ms=440
cell trial=window machine=hosted-win arm=b run=5 key_bytes=19714048 second_bytes=4210688 helpers=0 cpu_ms=0 ticks=- frames=- load_x100=- cpu_pct=3 counted=yes show_ms=438
cell trial=window machine=hosted-win arm=b run=6 key_bytes=19021824 second_bytes=4292608 helpers=0 cpu_ms=0 ticks=- frames=- load_x100=- cpu_pct=3 counted=yes show_ms=440
cell trial=window machine=hosted-win arm=b run=7 key_bytes=18345984 second_bytes=4206592 helpers=0 cpu_ms=0 ticks=- frames=- load_x100=- cpu_pct=3 counted=yes show_ms=460
cell trial=window machine=hosted-win arm=b run=8 key_bytes=21450752 second_bytes=4190208 helpers=0 cpu_ms=31 ticks=- frames=- load_x100=- cpu_pct=30 counted=yes show_ms=560
cell trial=window machine=hosted-win arm=b run=9 key_bytes=20500480 second_bytes=4214784 helpers=0 cpu_ms=0 ticks=- frames=- load_x100=- cpu_pct=26 counted=yes show_ms=529
cell trial=window machine=hosted-win arm=b run=10 key_bytes=25632768 second_bytes=4276224 helpers=0 cpu_ms=0 ticks=- frames=- load_x100=- cpu_pct=16 counted=yes show_ms=532
figure trial=window machine=hosted-mac arm=main median_bytes=122748928 range_bytes=2375680 runs=10
figure trial=window machine=hosted-mac arm=a median_bytes=122691584 range_bytes=2424832 runs=10
figure trial=window machine=hosted-mac arm=a2 median_bytes=122716160 range_bytes=2523136 runs=10
figure trial=window machine=hosted-mac arm=b median_bytes=86048768 range_bytes=2064384 runs=10
figure trial=window machine=hosted-win arm=main median_bytes=146130944 range_bytes=3280896 runs=10
figure trial=window machine=hosted-win arm=a median_bytes=135831552 range_bytes=757760 runs=9
figure trial=window machine=hosted-win arm=a2 median_bytes=106332160 range_bytes=1507328 runs=10
figure trial=window machine=hosted-win arm=b median_bytes=19431424 range_bytes=7286784 runs=10
verdict window hosted_mac=hidden hosted_win=destroy quiet_mac=no quiet_win=-
