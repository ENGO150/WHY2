#ifndef WHY2_MISC_H
#define WHY2_MISC_H

#include <sys/time.h>

#include <why2/flags.h>

void generateTextKeyChain(char *key, int *textKeyChain, int textKeyChainSize); //GENERATES ARRAY FOR ENCRYPTION/DECRYPTION
void deallocateOutput(outputFlags flags); //DEALLOCATES flags
boolean checkVersion(); //THIS FUNCTION CHECKS IF LATEST VERSION OF WHY2 IS USED
boolean checkKey(char *key); //CHECKS IF KEY IS VALID
boolean checkText(char *text); //CHECKS IF TEXT IS VALID
unsigned long countIntLength(int number); //RETURNS LENGTH OF number
unsigned long countUnusedKeySize(char *text, char *key); //COUNT unusedKeySize
unsigned long countRepeatedKeySize(char *text, char *key); //COUNT repeatedKeySize
unsigned long compareTimeMicro(struct timeval startTime, struct timeval finishTime); //COMPARE TIMES IN MICROSECONDS

#endif
