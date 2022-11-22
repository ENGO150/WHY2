#include <why2.h>

#include <stdio.h>

int main(void)
{
    //VARIABLES
    int test = initLogger(WRITE_DIR); //INITIALIZE LOGGER FILE

    printf("%d\n", test);

    writeLog(test, WRITE_MESSAGE); //WRITE

    //DEALLOCATION
    deallocateLogger(test);
    return 0;
}