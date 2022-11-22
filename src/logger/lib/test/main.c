#include <why2.h>

#include <stdio.h>

int main(void)
{
    //VARIABLES
    int logFile = initLogger(WRITE_DIR); //INITIALIZE LOGGER FILE

    printf("%d\n", logFile);

    writeLog(logFile, WRITE_MESSAGE); //WRITE

    //DEALLOCATION
    deallocateLogger(logFile);
    return 0;
}