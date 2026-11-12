#include <mach/mach.h>
#include <servers/bootstrap.h>
#include <stdio.h>

#ifndef BUILD_THAT_NEVER_RAN
#error "gate/fixtures/outbound/attempt.sh builds this file and names each build in BUILD_THAT_NEVER_RAN"
#endif

int main(void) {
  mach_port_t service = MACH_PORT_NULL;
  kern_return_t result = bootstrap_look_up(bootstrap_port, "com.apple.dnssd.service", &service);
  printf("mach lookup of the DNS service (%s): %s\n", BUILD_THAT_NEVER_RAN, result == KERN_SUCCESS ? "found" : "refused");
  return 0;
}
