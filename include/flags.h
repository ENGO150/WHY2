#ifndef WHY2_FLAGS_H
#define WHY2_FLAGS_H

//CONSTS
#define ENCRYPTION_SEPARATOR '.' //SEPARATOR BETWEEN KEYS
#define ENCRYPTION_SEPARATOR_STRING "." //SAME AS ENCRYPTION_SEPARATOR BUT AS STRING

#define INVALID_KEY 1 //EXIT VALUE FOR INVALID KEY
#define DOWNLOAD_FAILED 1 //EXIT VALUE FOR versions.json DOWNLOAD FAILED

#define VERSION "v3.2" //VERSION OF CURRENT BUILD     > DO NOT TOUCH THIS <
#define VERSIONS_URL "https://raw.githubusercontent.com/ENGO150/WHY2/stable/versions.json" //URL FOR GETTING versions.json
#define VERSIONS_NAME "versions.json" //do I have to explain this?

#define TEST_TEXT "Pepa smrdí." //TEST TEXT FOR ENCRYPTION IN why2-test BINARY
#define TEST_KEY "lZwOBFvjJEmaYRIaKsALKLkSeJvXhFPbZIRNFbjQRNyiOuLTexhgOpObHzyQgNT" //TEST KEY FOR ENCRYPTION IN why2-test BINARY

#define TEXT_TO_ENCRYPT "Some text yk" //THIS TEXT WILL BE ENCRYPTED IN why2-app BINARY

#define CLEAR_SCREEN "\e[1;1H\e[2J" //TEXT FOR UNIX CLEAR SCREEN

#define NOT_FOUND_TRIES 10 //NUMBER OF TRIES FOR DOWNLOADING versions.json

#define DEPRECATED __attribute__((deprecated))

typedef struct
{
    int skipCheck;
    int noOutput;
} inputFlags;

typedef struct
{
    char *outputText;
    char *usedKey;
} *outputFlags;

//VARIABLES
static int keyLength = 50; //LENGTH OF KEY     > DO NOT TOUCH THIS <

static int skipCheck = 0; //BOOLEAN FOR SKIPPING VERSION CHECK
static int noOutput = 0; //BOOLEAN FOR NOT PRINTING OUTPUT WHEN ENCRYPTING/DECRYPTING

//GETTERS
int getKeyLength();
DEPRECATED int getSkipCheck();
DEPRECATED int getNoOutput();

//SETTERS
void setKeyLength(int keyLengthNew);
DEPRECATED void setSkipCheck(int skipCheckNew);
DEPRECATED void setNoOutput(int noOutputNew);

#endif
