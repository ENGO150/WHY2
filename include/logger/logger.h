#ifndef WHY2_LOGGER_LOGGER_H
#define WHY2_LOGGER_LOGGER_H

int initLogger(char *directoryPath); //CREATES LOGGING FILE IN directoryPath
void writeLog(int loggerFile, char *logMessage); //WRITES logMessage TO loggerFile

#endif