/* THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!! */

// Copyright 2020-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

package com.example.godotwry

import com.example.godotwry.RustWebView
import android.annotation.SuppressLint
import android.os.Build
import android.os.Bundle
import android.webkit.WebView
import android.view.KeyEvent
import org.godotengine.godot.GodotActivity

open class WryActivity : GodotActivity() {
    private lateinit var mWebView: RustWebView

    companion object {
        init {
            // Force load libgodot_wry.so BEFORE onCreate() runs.
            // This ensures wry::android_setup() can be called immediately from onCreate().
            // The library is in the standard Android lib directory searched by the runtime.
            System.loadLibrary("godot_wry")
        }
    }

    open fun onWebViewCreate(webView: WebView) {
        runOnUiThread {
            val params = android.widget.FrameLayout.LayoutParams(
                android.view.ViewGroup.LayoutParams.MATCH_PARENT,
                android.view.ViewGroup.LayoutParams.MATCH_PARENT
            )
            addContentView(webView, params)
            webView.setBackgroundColor(android.graphics.Color.TRANSPARENT)
        }
    }

    fun setWebView(webView: RustWebView) {
        mWebView = webView
        onWebViewCreate(webView)
    }

    val version: String
        @SuppressLint("WebViewApiAvailability", "ObsoleteSdkInt")
        get() {
            // Check getCurrentWebViewPackage() directly if above Android 8
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                return WebView.getCurrentWebViewPackage()?.versionName ?: ""
            }

            // Otherwise manually check WebView versions
            var webViewPackage = "com.google.android.webview"
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
              webViewPackage = "com.android.chrome"
            }
            try {
                @Suppress("DEPRECATION")
                val info = packageManager.getPackageInfo(webViewPackage, 0)
                return info.versionName.toString()
            } catch (ex: Exception) {
                Logger.warn("Unable to get package info for '$webViewPackage'$ex")
            }

            try {
                @Suppress("DEPRECATION")
                val info = packageManager.getPackageInfo("com.android.webview", 0)
                return info.versionName.toString()
            } catch (ex: Exception) {
                Logger.warn("Unable to get package info for 'com.android.webview'$ex")
            }

            // Could not detect any webview, return empty string
            return ""
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Call into native code to run wry::android_setup() on the UI thread.
        // This is safe because by the time onCreate runs, Godot has already
        // loaded libgodot_wry.so through the GDExtension loader.
        try {
            nativeInit(this)
        } catch (e: UnsatisfiedLinkError) {
            // If the native library hasn't been loaded yet (e.g. first launch
            // before GDExtension initialisation), silently skip — the GDExtension
            // node will retry initialisation from its ready() callback.
            Logger.warn("nativeInit skipped: ${e.message}")
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        try {
            onActivityDestroy()
        } catch (_: UnsatisfiedLinkError) { }
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent?): Boolean {
        if (::mWebView.isInitialized && keyCode == KeyEvent.KEYCODE_BACK && mWebView.canGoBack()) {
            mWebView.goBack()
            return true
        }
        return super.onKeyDown(keyCode, event)
    }

    fun getAppClass(name: String): Class<*> {
        return Class.forName(name)
    }

    // Called from onCreate to pass the Activity to wry::android_setup()
    private external fun nativeInit(activity: WryActivity)
    private external fun onActivityDestroy()

    
}
