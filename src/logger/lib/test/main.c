#include <why2.h>

#include <stdio.h>
#include <unistd.h>

int main(void)
{
    int test = initLogger("./logs");

    printf("%d\n", test);

    close(test);
    return 0;
}