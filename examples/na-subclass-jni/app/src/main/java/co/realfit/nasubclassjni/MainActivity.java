package co.realfit.nasubclassjni;

import android.app.NativeActivity;
import android.content.Intent;
import android.os.Bundle;

public class MainActivity extends NativeActivity {

    static {
        System.loadLibrary("na_subclass_jni");
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);

        notifyOnNewIntent();
    }

    private native void notifyOnNewIntent();
}
