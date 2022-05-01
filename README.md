# WHY2 Encryption System

*Yeah.*

### Table of contents

  - [Installation](#installation)
  - [Using in Your projects](#using-in-your-projects)
  - [Example of code](#example-of-code)
  - [Example programs](#example-programs)
  - WHY2 in another languages
    - [JavaScript](#javascript)

## Installation

Run 'build.sh install' with root permissions to install WHY2 on your system.

`sudo ./build.sh install`
## Using in Your projects 

Run `configure.sh` and you'll be good to go.

To **encrypt** text, use function `encryptText()` from file `<why2/encrypter.h>` (`include/encrypter.h`).

To **decrypt** text, use function `decryptText()` from file `<why2/decrypter.h>` (`include/decrypter.h`).

Jump to [examples](#examples) if you're not sure, how to use.

## Example of code

- Encryption:
    ```c
    //FIRST VARIANT
    char *yourText = encryptText("Put here text, you want encrypt...", "tzXlZGxkhfYOvRthqokDrmGFyDMylgmeIlrJTpVAwuqrLjABXM");
    //The second thing is Your **key**. (The key must be at least 50 characters long!)

    //SECOND VARIANT
    char *yourText = encryptText("Put here text, you want encrypt...", NULL);
    //See? You don't have to use Your key. Program will automatically generate one for you. It will be printed out, so save it somewhere.
    ```

- Decryption:
    ```c
    char *yourText = decryptText("158.-83.9388.-14.57.8419.113.-98.10576", "tzXlZGxkhfYOvRthqokDrmGFyDMylgmeIlrJTpVAwuqrLjABXM");
    //First parameter is Your encrypted text, the second is key you want to use for decryption it.
    ```

## Example programs

Uhm... There aren't any examples (for now)... We will maybe create some... Later...

## JavaScript

You can found the JS remake [>> RIGHT HERE <<](https://github.com/ENDev-WHY2/WHY2-Encryption-System/tree/js). Thanks, [@SebestikCZ](https://github.com/SebestikCZ) <3
