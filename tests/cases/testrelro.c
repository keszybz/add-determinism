#include <stdio.h>

extern char _GLOBAL_OFFSET_TABLE_;

int main() {
  printf("Hello world\n");
  _GLOBAL_OFFSET_TABLE_ = 3;
}
