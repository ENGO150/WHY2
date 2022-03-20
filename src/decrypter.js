module.exports = (text, key) => {
	const KEY_LENGTH = 50;
        const ENCRYPTION_SEPARATOR = ".";

        //CHECK FOR INVALID key
        if (key.length < KEY_LENGTH) {
            return { exitCode: -2 };
        }

        //VARIABLES
        var returningText;
        var numberBuffer;
        var textBuffer = "";
        var textBuffer1 = "";

        numberBuffer = 1;

        //GET LENGTH OF returningText AND textKeyChain
        for (var i = 0; i < text.length; i++) {
            if (text[i] == ENCRYPTION_SEPARATOR) numberBuffer++;
        }

        //SET LENGTH
        returningText = [];
        returningText.length = numberBuffer;
        var textKeyChain = [];
        var encryptedTextKeyChain = [];
        textKeyChain.length = encryptedTextKeyChain.length = numberBuffer;

        //LOAD textKeyChain
        for (var i = 0; i < textKeyChain.length; i++) {
            numberBuffer = i;

            //CHECK, IF numberBuffer ISN'T GREATER THAN KEY_LENGTH AND CUT UNUSED LENGTH
            while (numberBuffer >= key.length) {
                numberBuffer -= key.length;
            }
            if (typeof key === 'string') {
                key = key.split("");
                key = key.map(element => element.charCodeAt(0));
            }

            //FILL textKeyChain
            if ((numberBuffer + 1) % 3 == 0)
            {
                textKeyChain[i] = key[numberBuffer] * key[numberBuffer + 1];
            } else if ((numberBuffer + 1) % 2 == 0)
            {
                textKeyChain[i] = key[numberBuffer] - key[numberBuffer + 1];
            } else
            {
                textKeyChain[i] = key[numberBuffer] + key[numberBuffer + 1];
            }
        }

        //LOAD encryptedTextKeyChain
        for (var i = 0; i < encryptedTextKeyChain.length; i++) {
            numberBuffer = 0;

            textBuffer1 = text.split(ENCRYPTION_SEPARATOR);

            try {
                textBuffer = textBuffer1[i];
                encryptedTextKeyChain[i] = parseInt(textBuffer);
            } catch (e) {
                return { exitCode: -1, error: e };
            }
            textBuffer = "";
        }

        //DECRYPT TEXT
        for (var i = 0; i < textKeyChain.length; i++) {
            textKeyChain[i] -= encryptedTextKeyChain[i];
        }

        //LOAD returningText
        for (var i = 0; i < textKeyChain.length; i++) {
            returningText[i] = String.fromCharCode(textKeyChain[i]);
        }

        returningText = returningText.join("");
        return { exitCode: 0, value: returningText };
    },