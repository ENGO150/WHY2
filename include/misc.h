#ifndef WHY2_MISC_H
#define WHY2_MISC_H

#include <sys/time.h>

#include <why2/flags.h>

void generateTextKeyChain(char *key, int *textKeyChain, int textKeyChainSize); //GENERATES ARRAY FOR ENCRYPTION/DECRYPTION
void deallocateOutput(outputFlags flags); //DEALLOCATES flags
unsigned char checkVersion(inputFlags flags); //THIS FUNCTION CHECKS IF LATEST VERSION OF WHY2 IS USED
unsigned char checkKey(char *key, inputFlags flags); //CHECKS IF KEY IS VALID
unsigned char checkText(char *text, inputFlags flags); //CHECKS IF TEXT IS VALID
unsigned long countIntLength(int number); //RETURNS LENGTH OF number
unsigned long countUnusedKeySize(char *text, char *key); //COUNT unusedKeySize
unsigned long compareTimeMicro(struct timeval startTime, struct timeval finishTime); //COMPARE TIMES IN MICROSECONDS

#endif
