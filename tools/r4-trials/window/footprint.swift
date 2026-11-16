// The physical footprint of processes on macOS, built and run by sample.sh.
// usage: footprint <pid>...: one line "<pid> <bytes>" per process that proc_pid_rusage reads.
import Darwin

for arg in CommandLine.arguments.dropFirst() {
  guard let pid = pid_t(arg) else {
    fputs("REFUSED usage: footprint <pid>...\n", stderr)
    exit(2)
  }
  var info = rusage_info_v4()
  let rc = withUnsafeMutablePointer(to: &info) { pointer in
    pointer.withMemoryRebound(to: rusage_info_t?.self, capacity: 1) { proc_pid_rusage(pid, RUSAGE_INFO_V4, $0) }
  }
  if rc == 0 {
    print("\(pid) \(info.ri_phys_footprint)")
  }
}
