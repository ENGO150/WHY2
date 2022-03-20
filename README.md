# WHY2 Encryption System

### Table of contents

  - [Using in Your projects](#using-in-your-projects)
  - [Example of code](#example-of-code)
  - [Example programs](#example-programs)

## Using in Your projects 

To **encrypt** text, use function `encrypt()` from file `./index.js`.

To **decrypt** text, use function `decrypt()` from file `./index.js`.

~~Jump to [examples](#examples) if you're not sure, how to use.~~

## Example of code

- Encryption:
    ```js
    const { encrypt } = require("why2-encryption-system.js");
    //FIRST VARIANT
    var yourText = await encrypt("Put here text, you want encrypt...", "tzXlZGxkhfYOvRthqokDrmGFyDMylgmeIlrJTpVAwuqrLjABXM"); //The second thing is Your **key**. (The key must be atleast 50 characters long!)

    //SECOND VARIANT
    var yourText = await encrypt("Put here text, you want encrypt..."); //See? You don't have to use Your key. Program will automatically generate one for you.
    ```
**WARNING!** The key from encryption will be printed out along the text value as an object \
**The method needs to be awaited, due to `crypto` randomizer.**
_Note: exit codes \
`-2` - invalid key \
`0` - operation completed successfully \
`-1` - operation failed, description available using error property_

- Decryption:
    ```js
	const { decrypt } = require("why2-encryption-system.js");
	var yourText = decrypt("158.-83.9388.-14.57.8419.113.-98.10576", "tzXlZGxkhfYOvRthqokDrmGFyDMylgmeIlrJTpVAwuqrLjABXM"); //First parameter is Your encrypted text, the second is key you want to use for decryption.
    ```

## Example programs

Uhm... There aren't any examples (for now)... I will maybe create some... Later...
