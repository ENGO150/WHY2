#ifndef WHY2_FLAGS_H
#define WHY2_FLAGS_H

//CONSTS
#define SUCCESS 0 //EXIT CODE FOR SUCCESSFUL RUN
#define INVALID_KEY 1 //EXIT VALUE FOR INVALID KEY
#define INVALID_TEXT 4 //EXIT VALUE FOR INVALID TEXT
#define DOWNLOAD_FAILED 2 //EXIT VALUE FOR versions.json DOWNLOAD FAILED
#define UPDATE_FAILED 3 //EXIT VALUE FOR UPDATE FAILED

#define VERSION "v4.3" //VERSION OF CURRENT BUILD     > DO NOT TOUCH THIS <
#define VERSIONS_URL "https://raw.githubusercontent.com/ENGO150/WHY2/release/versions.json" //URL FOR GETTING versions.json
#define VERSIONS_NAME "/tmp/why2-versions.json" //do I have to explain this?

#define UPDATE_URL "https://github.com/ENGO150/WHY2.git" // REPOSITORY URL FOR UPDATES (YOU DON'T SAY)
#define UPDATE_NAME "/tmp/why2-update" // fuck you
#define UPDATE_COMMAND "tmux new-session -d \"cd {DIR} && make install\""

#define TEST_TEXT "aAzZ(    )!?#\\/śŠ <3|420*;" //TEST TEXT FOR ENCRYPTION IN why2-test BINARY

#define TEXT_TO_ENCRYPT "Some text yk" //THIS TEXT WILL BE ENCRYPTED IN why2-app BINARY

#define CLEAR_SCREEN "\e[1;1H\e[2J" //TEXT FOR UNIX CLEAR SCREEN

#define NOT_FOUND_TRIES 10 //NUMBER OF TRIES FOR DOWNLOADING versions.json

#define DEPRECATED __attribute__((deprecated)) //SAME COMMENT AS VERSIONS_NAME'S
#define UNUSED __attribute__((unused)) //SAME COMMENT AS DEPRECATED'S

typedef struct
{
    unsigned char noCheck; //BOOLEAN FOR SKIPPING VERSION CHECK
    unsigned char noOutput; //BOOLEAN FOR NOT PRINTING OUTPUT WHEN ENCRYPTING/DECRYPTING
    unsigned char update; //BOOLEAN FOR UPDATING YOUR WHY VERSION IF OLD IS USED
} inputFlags;

typedef struct
{
    char *outputText; //VARIABLE FOR ENCRYPTED/DECRYPTED TEXT
    char *usedKey; //VARIABLE FOR USED/GENERATED KEY
    unsigned long unusedKeySize; //VARIABLE FOR COUNT OF UNUSED CHARACTERS IN KEY
    unsigned long elapsedTime; //VARIABLE FOR ELAPSED TIME IN MICROSECONDS => 1s = 1000000µs
    unsigned char exitCode; //VARIABLE FOR EXIT CODE
} outputFlags;

//NOTE: Variables were moved to 'flags.c' to force y'all using getters

//GETTERS
char getEncryptionSeparator();
unsigned long getKeyLength();
inputFlags defaultFlags(); //THIS GENERATES inputFlags WITH DEFAULT VALUES
inputFlags getFlags(); //RETURNS USED FLAGS
outputFlags noOutput(unsigned char exitCode); //SAME AS defaultFlags() BUT FOR outputFlags

//SETTERS
void setEncryptionSeparator(char encryptionSeparatorNew);
void setKeyLength(int keyLengthNew);
void setFlags(inputFlags newFlags); //.... whatcha think?

#endif
