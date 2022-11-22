#include <why2/logger/utils.h>

#include <unistd.h>

void deallocateLogger(int logger)
{
    close(logger);
}