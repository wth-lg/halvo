#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#ifndef BUILD_THAT_NEVER_RAN
#error "gate/fixtures/outbound/attempt.sh builds this file and names each build in BUILD_THAT_NEVER_RAN"
#endif

int main(int argc, char **argv) {
  struct sockaddr_in to = {.sin_family = AF_INET, .sin_port = htons(9)};
  if (argc != 2 || inet_pton(AF_INET, argv[1], &to.sin_addr) != 1) return 64;
  int s = socket(AF_INET, SOCK_STREAM, 0);
  if (s < 0) return 71;
  int result = connect(s, (struct sockaddr *)&to, sizeof to);
  printf("native connect (%s): %s\n", BUILD_THAT_NEVER_RAN, result == 0 ? "connected" : strerror(errno));
  close(s);
  return 0;
}
