// https://github.com/QingdaoU/Judger/blob/newnew/tests/test_src/integration/memory3.c
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int arr[102400000];


int main()
{
    memset(arr, 1, sizeof(arr));
    return 0;
}