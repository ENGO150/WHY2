#ifndef WHY2_FLAGS_H
#define WHY2_FLAGS_H

//CONSTS
#define KEY_LENGTH 50
#define ENCRYPTION_SEPARATOR '.'
#define ENCRYPTION_SEPARATOR_STRING "."

#define INVALID_KEY 1

#define VERSION "v3.0"
#define VERSIONS_URL "https://raw.githubusercontent.com/ENGO150/WHY2/master/versions.json"
#define VERSIONS_NAME "versions.json"

//VARIABLES
static int skipCheck = 0;

//FUNCTIONS
int getSkipCheck();
void setSkipCheck(int skipCheckNew);

#endif
