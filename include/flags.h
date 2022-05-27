#ifndef WHY2_FLAGS_H
#define WHY2_FLAGS_H

//CONSTS
#define ENCRYPTION_SEPARATOR '.' //SEPARATOR BETWEEN KEYS
#define ENCRYPTION_SEPARATOR_STRING "." //SAME AS ENCRYPTION_SEPARATOR BUT AS STRING

#define INVALID_KEY 1 //EXIT VALUE FOR INVALID KEY
#define DOWNLOAD_FAILED 2 //EXIT VALUE FOR versions.json DOWNLOAD FAILED

#define VERSION "v4.0.1" //VERSION OF CURRENT BUILD     > DO NOT TOUCH THIS <
#define VERSIONS_URL "https://raw.githubusercontent.com/ENGO150/WHY2/stable/versions.json" //URL FOR GETTING versions.json
#define VERSIONS_NAME "versions.json" //do I have to explain this?

#define TEST_TEXT "aAzZ(    )!?#\\/śŠ 420*;" //TEST TEXT FOR ENCRYPTION IN why2-test BINARY

#define TEXT_TO_ENCRYPT "Some text yk" //THIS TEXT WILL BE ENCRYPTED IN why2-app BINARY

#define CLEAR_SCREEN "\e[1;1H\e[2J" //TEXT FOR UNIX CLEAR SCREEN

#define NOT_FOUND_TRIES 10 //NUMBER OF TRIES FOR DOWNLOADING versions.json

#define DEPRECATED __attribute__((deprecated)) //SAME COMMENT AS VERSIONS_NAME'S

typedef struct
{
    unsigned char skipCheck; //BOOLEAN FOR SKIPPING VERSION CHECK
    unsigned char noOutput; //BOOLEAN FOR NOT PRINTING OUTPUT WHEN ENCRYPTING/DECRYPTING
} inputFlags;

typedef struct
{
    char *outputText; //VARIABLE FOR ENCRYPTED/DECRYPTED TEXT
    char *usedKey; //VARIABLE FOR USED/GENERATED KEY
} outputFlags;

//VARIABLES
static int keyLength = 50; //LENGTH OF KEY     > DO NOT TOUCH THIS <

//GETTERS
int getKeyLength();
inputFlags noFlags(); //THIS GENERATES inputFlags WITH DEFAULT VALUES

//SETTERS
void setKeyLength(int keyLengthNew);

#endif
