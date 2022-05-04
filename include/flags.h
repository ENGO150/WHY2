#ifndef WHY2_FLAGS_H
#define WHY2_FLAGS_H

//CONSTS
#define ENCRYPTION_SEPARATOR '.'
#define ENCRYPTION_SEPARATOR_STRING "."

#define INVALID_KEY 1

#define VERSION "v3.1"
#define VERSIONS_URL "https://raw.githubusercontent.com/ENGO150/WHY2/stable/versions.json"
#define VERSIONS_NAME "versions.json"

//VARIABLES
static int keyLength = 50;

static int skipCheck = 0;
static int noOutput = 0;

//GETTERS
int getSkipCheck();
int getKeyLength();
int getNoOutput();

//SETTERS
void setSkipCheck(int skipCheckNew);
void setKeyLength(int keyLengthNew);
void setNoOutput(int noOutputNew);

#endif
