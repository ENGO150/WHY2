#ifndef WHY2_FLAGS_H
#define WHY2_FLAGS_H

//CONSTS
#define ENCRYPTION_SEPARATOR '.' //SEPARATOR BETWEEN KEYS
#define ENCRYPTION_SEPARATOR_STRING "." //SAME AS ENCRYPTION_SEPARATOR BUT AS STRING

#define INVALID_KEY 1 //EXIT VALUE FOR INVALID KEY

#define VERSION "v3.2" //VERSION OF CURRENT BUILD     > DO NOT TOUCH THIS <
#define VERSIONS_URL "https://raw.githubusercontent.com/ENGO150/WHY2/stable/versions.json" //URL FOR GETTING versions.json
#define VERSIONS_NAME "versions.json" //do I have to explain this?

//VARIABLES
static int keyLength = 50; //LENGTH OF KEY     > DO NOT TOUCH THIS <

static int skipCheck = 0; //BOOLEAN FOR SKIPPING VERSION CHECK
static int noOutput = 0; //BOOLEAN FOR NOT PRINTING OUTPUT WHEN ENCRYPTING/DECRYPTING

//GETTERS
int getSkipCheck();
int getKeyLength();
int getNoOutput();

//SETTERS
void setSkipCheck(int skipCheckNew);
void setKeyLength(int keyLengthNew);
void setNoOutput(int noOutputNew);

#endif
