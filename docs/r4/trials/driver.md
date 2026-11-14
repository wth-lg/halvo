Read on every target and not on the four built targets alone, licences would be miss for both candidates, because each adds target-lexicon 0.13.5 under Apache-2.0 WITH LLVM-exception as a build dependency on Linux, and the verdict would then be none on both machines.

reading trial=driver machine=mac candidate=webdriver version=0.2.4 build=ok licences=ok input_calls=0 banned=1
reading trial=driver machine=mac candidate=webdriver entry=target-lexicon version=0.13.5 licence=Apache-2.0_WITH_LLVM-exception built_targets=- rulings=P7-52,CP-102
reading trial=driver machine=mac candidate=wdio version=1.5.1 build=ok licences=miss input_calls=0 banned=1
reading trial=driver machine=mac candidate=wdio entry=target-lexicon version=0.13.5 licence=Apache-2.0_WITH_LLVM-exception built_targets=- rulings=P7-52,CP-102
cell trial=driver machine=hosted-mac candidate=webdriver sessions=5 ok=5
cell trial=driver machine=hosted-mac candidate=wdio sessions=5 ok=5
cell trial=driver machine=hosted-win candidate=webdriver sessions=5 ok=5
cell trial=driver machine=hosted-win candidate=wdio sessions=5 ok=5
verdict driver hosted_mac=webdriver hosted_win=webdriver
