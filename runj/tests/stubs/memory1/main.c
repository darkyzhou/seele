// https://github.com/QingdaoU/Judger/blob/newnew/tests/test_src/integration/memory1.c
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main()
{
    int size = 80 * 1024 * 1024;
    int *a = NULL;
    a = (int *)malloc(size);
    memset(a, 1, size);
    free(a);
    return 0;
}