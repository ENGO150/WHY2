#include <why2.h>

#include <stdio.h>

int main(void)
{
    int test = initLogger("./logs");

    printf("%d\n", test);

    writeLog(test, "test");

    deallocateLogger(test);
    return 0;
}